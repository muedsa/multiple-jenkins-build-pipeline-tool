//! 针对 Jenkins REST 交互的集成测试。
//!
//! 这里跑一个最小的假 Jenkins，用来验证真实流程里的关键环节：
//! 取 CSRF crumb、带参数触发构建、从队列项解析构建号、轮询直到结束，
//! 以及各种失败路径（401 / 404 / 构建失败 / 超时）。
//!
//! 需要写系统凭据管理器的用例默认跳过，见 [`skip_without_keyring`]。

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use jenkins_build_pipeline_tool_lib::config::StageConfig;
use jenkins_build_pipeline_tool_lib::jenkins::{BuildState, JenkinsClient};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// 假 Jenkins 的可配置行为。
#[derive(Default)]
struct FakeJenkins {
    /// 收到触发请求时返回的状态码，默认 201。
    trigger_status: Option<u16>,
    /// 所有响应统一返回的状态码（模拟认证失败等）。
    force_status: Option<u16>,
    /// 队列项需要几次查询才给出构建号，用来模拟排队。
    queue_ready_after: usize,
    /// 构建需要几次查询才结束，用来模拟执行中。
    build_finishes_after: usize,
    /// 构建的最终结果。
    final_result: Option<String>,
    /// 是否返回 crumb。
    with_crumb: bool,
}

/// 假服务器的运行状态。
struct FakeState {
    config: FakeJenkins,
    /// 队列项被查询过几次。
    queue_hits: AtomicUsize,
    /// 构建状态被查询过几次。
    build_hits: AtomicUsize,
    /// 记录收到过的请求路径，便于断言。
    requests: std::sync::Mutex<Vec<String>>,
    /// 记录触发请求的表单体。
    trigger_body: std::sync::Mutex<String>,
    /// 期望收到的 Authorization 头。
    expected_auth: String,
}

/// 启动假 Jenkins，返回 base URL 与共享状态。
async fn start_fake(config: FakeJenkins, expected_auth: &str) -> (String, Arc<FakeState>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let state = Arc::new(FakeState {
        config,
        queue_hits: AtomicUsize::new(0),
        build_hits: AtomicUsize::new(0),
        requests: std::sync::Mutex::new(Vec::new()),
        trigger_body: std::sync::Mutex::new(String::new()),
        expected_auth: expected_auth.to_string(),
    });

    let shared = state.clone();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            let state = shared.clone();
            tokio::spawn(async move {
                let _ = serve(stream, state).await;
            });
        }
    });

    (format!("http://{addr}"), state)
}

/// 极简 HTTP/1.1 处理：只解析请求行、少数头部和 Content-Length 正文。
async fn serve(mut stream: TcpStream, state: Arc<FakeState>) -> std::io::Result<()> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    // 读到头部结束。
    let header_end = loop {
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(pos) = find_subslice(&buf, b"\r\n\r\n") {
            break pos + 4;
        }
    };

    let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let mut lines = head.lines();
    let request_line = lines.next().unwrap_or_default().to_string();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("GET").to_string();
    let path = parts.next().unwrap_or("/").to_string();

    let mut content_length = 0usize;
    let mut authorization = String::new();
    for line in lines {
        let lower = line.to_ascii_lowercase();
        if let Some(value) = lower.strip_prefix("content-length:") {
            content_length = value.trim().parse().unwrap_or(0);
        }
        if lower.starts_with("authorization:") {
            authorization = line[14..].trim().to_string();
        }
    }

    // 继续读满正文。
    let mut body = buf[header_end..].to_vec();
    while body.len() < content_length {
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    let body_len = body.len().min(content_length);
    let body = String::from_utf8_lossy(&body[..body_len]).to_string();

    state
        .requests
        .lock()
        .unwrap()
        .push(format!("{method} {path}"));
    if path.contains("build") && method == "POST" {
        *state.trigger_body.lock().unwrap() = body.clone();
    }

    // 统一状态码优先（模拟认证/权限问题）。
    if let Some(code) = state.config.force_status {
        return respond(&mut stream, code, "text/plain", "forced").await;
    }

    // 校验 basic auth，确保客户端真的带上了凭据。
    if authorization != state.expected_auth {
        return respond(&mut stream, 401, "text/plain", "unauthorized").await;
    }

    let json = |s: &str| ("application/json".to_string(), s.to_string());
    // 查询串要剥掉再匹配，否则 /queue/item/1/api/json?tree=... 会落到 404 分支。
    let route = path.split('?').next().unwrap_or("").to_string();

    let (code, content_type, payload) = if path.contains("tree=jobs") {
        // 列表接口：带文件夹层级的一小段 Job 树。
        let (ct, b) = json(
            r####"{"jobs":[
              {"_class":"hudson.model.FreeStyleProject","name":"fsk-esale","fullName":"fsk-esale","url":"http://x/job/fsk-esale/"},
              {"_class":"org.jenkinsci.plugins.workflow.job.WorkflowJob","name":"pipe-a","fullName":"pipe-a","url":"http://x/job/pipe-a/"},
              {"_class":"com.cloudbees.hudson.plugins.folder.Folder","name":"team-a","fullName":"team-a","url":"http://x/job/team-a/","jobs":[
                {"_class":"hudson.model.FreeStyleProject","name":"svc-1","fullName":"team-a/svc-1","url":"http://x/job/team-a/job/svc-1/"}
              ]}
            ]}"####,
        );
        (200, ct, b)
    } else if route.starts_with("/api/json") {
        let (ct, b) = json(r####"{"mode":"NORMAL","version":"2.528.3"}"####);
        (200, ct, b)
    } else if route.starts_with("/crumbIssuer/api/json") {
        if state.config.with_crumb {
            let (ct, b) = json(r####"{"crumb":"abc123","crumbRequestField":"Jenkins-Crumb"}"####);
            (200, ct, b)
        } else {
            (404, "text/plain".to_string(), "no crumb".to_string())
        }
    } else if route.starts_with("/job/missing") {
        // 放在触发分支之前，保证不存在的 Job 真的返回 404。
        (404, "text/plain".to_string(), "not found".to_string())
    } else if route.starts_with("/job/paramjob/buildWithParameters") {
        // 参数化 Job：接受 buildWithParameters。
        (201, "text/plain".to_string(), String::new())
    } else if route.starts_with("/job/paramjob/build") {
        // 参数化 Job 上用「无参触发」会被 Jenkins 拒绝 —— 复现用户遇到的报错。
        (
            400,
            "text/html".to_string(),
            "<html><head><title>Error 400 Nothing is submitted</title></head><body><h2>HTTP ERROR 400 Nothing is submitted</h2></body></html>".to_string(),
        )
    } else if route.starts_with("/job/demo/buildWithParameters") {
        // buildWithParameters 只有真的提交了参数才接受；
        // 空表单同样会被 Jenkins 以 400 Nothing is submitted 拒绝。
        let html_400 = (
            400,
            "text/html".to_string(),
            "<html><head><title>Error 400 Nothing is submitted</title></head><body><h2>HTTP ERROR 400 Nothing is submitted</h2></body></html>".to_string(),
        );
        if body.trim().is_empty() {
            html_400
        } else {
            (201, "text/plain".to_string(), String::new())
        }
    } else if route.contains("/build") && method == "POST" {
        let code = state.config.trigger_status.unwrap_or(201);
        (code, "text/plain".to_string(), String::new())
    } else if route.starts_with("/queue/item/1/api/json") {
        let hits = state.queue_hits.fetch_add(1, Ordering::SeqCst);
        if hits + 1 >= state.config.queue_ready_after {
            let (ct, b) =
                json(r####"{"executable":{"number":42,"url":"http://x/job/demo/42/"}}"####);
            (200, ct, b)
        } else {
            let (ct, b) = json(r####"{"why":"waiting for executor","executable":null}"####);
            (200, ct, b)
        }
    } else if route.starts_with("/job/demo/42/api/json") {
        let hits = state.build_hits.fetch_add(1, Ordering::SeqCst);
        if hits + 1 >= state.config.build_finishes_after {
            let result = state
                .config
                .final_result
                .clone()
                .unwrap_or_else(|| "SUCCESS".to_string());
            let (ct, b) = json(&format!(
                r####"{{"number":42,"building":false,"result":"{result}","duration":65000,"url":"http://x/job/demo/42/","displayName":"#42"}}"####
            ));
            (200, ct, b)
        } else {
            let (ct, b) = json(
                r####"{"number":42,"building":true,"result":null,"duration":0,"url":"http://x/job/demo/42/","displayName":"#42"}"####,
            );
            (200, ct, b)
        }
    } else if route.starts_with("/job/demo/lastBuild/api/json") {
        let (ct, b) = json(
            r####"{"number":41,"building":false,"result":"SUCCESS","duration":12000,"url":"http://x/job/demo/41/","displayName":"#41"}"####,
        );
        (200, ct, b)
    } else {
        (404, "text/plain".to_string(), "unknown".to_string())
    };

    // 触发构建时补上 Location 头，指向队列项（故意用相对路径，Jenkins 就是这样）。
    // 注意两个触发端点都要覆盖：build 与 buildWithParameters。
    let is_trigger = method == "POST"
        && (route.ends_with("/build") || route.ends_with("/buildWithParameters"));
    if is_trigger && code == 201 {
        let extra = "Location: /queue/item/1/\r\n";
        return respond_with(&mut stream, code, &content_type, &payload, extra).await;
    }
    respond(&mut stream, code, &content_type, &payload).await
}

async fn respond(
    stream: &mut TcpStream,
    code: u16,
    content_type: &str,
    body: &str,
) -> std::io::Result<()> {
    respond_with(stream, code, content_type, body, "").await
}

async fn respond_with(
    stream: &mut TcpStream,
    code: u16,
    content_type: &str,
    body: &str,
    extra_headers: &str,
) -> std::io::Result<()> {
    let response = format!(
        "HTTP/1.1 {code} {}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n{extra_headers}\r\n{body}",
        if code < 300 { "OK" } else { "ERR" },
        body.len()
    );
    stream.write_all(response.as_bytes()).await?;
    stream.flush().await
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// 集成测试需要在系统凭据管理器里写入测试 token。
///
/// 这会真实改动用户的「Windows 凭据」，所以默认跳过；
/// 想跑完整流程时设 `JENKINS_TOOL_KEYRING_TESTS=1`，并且**必须单线程**运行：
/// `cargo test -- --test-threads=1`。
///
/// 为什么必须单线程：Windows 凭据管理器的读写不是线程安全的，
/// 多个测试并发读写时偶发丢写（刚存进去马上读却读不到）。
/// 这是环境限制，不是被测代码的问题，所以这两个条件都满足才跑。
/// 测试用的条目名都以 `inttest-` 开头，跑完会自行清理。
fn keyring_tests_enabled() -> bool {
    std::env::var("JENKINS_TOOL_KEYRING_TESTS")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

/// 是否跑在单线程模式下（并发写凭据会丢写，见 [`keyring_tests_enabled`]）。
fn running_single_threaded() -> bool {
    std::thread::available_parallelism()
        .map(|n| n.get() == 1)
        .unwrap_or(false)
}

/// 未满足前置条件时打印提示并让用例直接返回。
fn skip_without_keyring() -> bool {
    if !keyring_tests_enabled() {
        eprintln!(
            "跳过：该用例需要写入系统凭据管理器，请设 JENKINS_TOOL_KEYRING_TESTS=1 后再运行"
        );
        return true;
    }
    if !running_single_threaded() {
        eprintln!(
            "跳过：凭据测试需要单线程运行（并发读写 Windows 凭据管理器会偶发丢写）。\
             请用 cargo test -- --test-threads=1"
        );
        return true;
    }
    false
}

/// 系统凭据管理器对并发访问不友好，这里再拿一把进程内的锁兜底。
static KEYRING_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// 写凭据时先拿锁，避免并发写导致偶发失败。
fn save_token_serial(stage: &str, token: &str) -> Result<(), String> {
    let _guard = KEYRING_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    jenkins_build_pipeline_tool_lib::secret::save_token(stage, token)
}

/// 构造一个指向假服务器的环境配置，并把 token 存进系统凭据管理器。
fn config_for(stage: &str, base_url: &str, token: &str) -> StageConfig {
    // 每次用不同的 token，避免和用户真实凭据撞车。
    save_token_serial(stage, token)
        .unwrap_or_else(|e| panic!("保存测试 token 失败（{stage}）：{e}"));
    let mut config = StageConfig::new(stage);
    config.base_url = base_url.to_string();
    config.username = "tester".to_string();
    config.poll_interval_secs = 1;
    config.timeout_minutes = 1;
    config
}

/// 测试结束后清理凭据管理器里的条目。
fn cleanup(stage: &str) {
    let _guard = KEYRING_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _ = jenkins_build_pipeline_tool_lib::secret::clear_token(stage);
}

/// 期望的 Authorization 头（basic base64("tester:token")）。
fn basic_auth(token: &str) -> String {
    format!(
        "Basic {}",
        base64_encode(format!("tester:{token}").as_bytes())
    )
}

fn base64_encode(input: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// 完整跑一遍 dev 侧流程：触发 -> 排队 -> 执行 -> 成功。
#[tokio::test]
async fn full_build_flow_succeeds() {
    if skip_without_keyring() {
        return;
    }
    let token = "inttest-token-full";
    let (base, state) = start_fake(
        FakeJenkins {
            queue_ready_after: 2,
            build_finishes_after: 3,
            with_crumb: true,
            ..Default::default()
        },
        &basic_auth(token),
    )
    .await;

    let config = config_for("inttest-full", &base, token);
    let client = JenkinsClient::new(&config).expect("构造客户端");

    let queue = client.trigger_build("demo", &[]).await.expect("触发构建");
    // Jenkins 返回的是相对路径，客户端应当补成绝对 URL 才能继续请求。
    assert!(
        queue.url.contains("/queue/item/1"),
        "队列地址：{}",
        queue.url
    );
    // 末尾多余的斜杠应当被去掉，否则拼查询串会变成 /1/?tree=...
    assert!(
        !queue.url.ends_with('/'),
        "队列地址不应以斜杠结尾：{}",
        queue.url
    );

    let build_number = client
        .resolve_build_number(
            &queue,
            Duration::from_millis(20),
            Duration::from_secs(10),
            || {},
        )
        .await
        .unwrap_or_else(|e| {
            let requests = state.requests.lock().unwrap().clone();
            panic!(
                "解析构建号失败：{e:?}；队列地址={}；已收到请求={requests:?}",
                queue.url
            )
        })
        .expect("应当拿到构建号");
    assert_eq!(build_number, 42);

    let status = client
        .wait_for_build(
            "demo",
            42,
            Duration::from_millis(20),
            Duration::from_secs(10),
            |_| {},
        )
        .await
        .expect("等待构建结束");
    assert_eq!(status.state, BuildState::Success);
    assert!(status.state.is_success());
    assert_eq!(status.build_number, 42);
    // 65000ms -> 65s
    assert_eq!(status.duration_secs, 65);

    // 确认确实经历了排队与轮询，而不是一次就返回。
    let requests = state.requests.lock().unwrap().clone();
    assert!(
        state.queue_hits.load(Ordering::SeqCst) >= 2,
        "队列应当被轮询多次，实际请求：{requests:?}"
    );
    assert!(
        state.build_hits.load(Ordering::SeqCst) >= 3,
        "构建状态应当被轮询多次，实际请求：{requests:?}"
    );
    assert!(
        requests
            .iter()
            .any(|p| p.starts_with("GET /queue/item/1/api/json")),
        "实际请求：{requests:?}"
    );

    cleanup("inttest-full");
}

/// 带参数触发时应当走 buildWithParameters 并提交参数。
#[tokio::test]
async fn trigger_with_params_uses_build_with_parameters() {
    if skip_without_keyring() {
        return;
    }
    let token = "inttest-token-params";
    let (base, state) = start_fake(
        FakeJenkins {
            queue_ready_after: 1,
            build_finishes_after: 1,
            with_crumb: true,
            ..Default::default()
        },
        &basic_auth(token),
    )
    .await;

    let config = config_for("inttest-params", &base, token);
    let client = JenkinsClient::new(&config).expect("构造客户端");

    let params = vec![
        jenkins_build_pipeline_tool_lib::config::BuildParam {
            key: "BRANCH".to_string(),
            value: "release/1.0".to_string(),
        },
        jenkins_build_pipeline_tool_lib::config::BuildParam {
            key: "DEPLOY".to_string(),
            value: "true".to_string(),
        },
    ];
    client
        .trigger_build("demo", &params)
        .await
        .expect("触发构建");

    let paths = state.requests.lock().unwrap().clone();
    assert!(
        paths.iter().any(|p| p.contains("buildWithParameters")),
        "应当走 buildWithParameters，实际请求：{paths:?}"
    );

    let body = state.trigger_body.lock().unwrap().clone();
    assert!(body.contains("BRANCH=release%2F1.0"), "表单体：{body}");
    assert!(body.contains("DEPLOY=true"), "表单体：{body}");

    cleanup("inttest-params");
}

/// 不带参数时走 build，避免无参 Job 报错。
#[tokio::test]
async fn trigger_without_params_uses_build() {
    if skip_without_keyring() {
        return;
    }
    let token = "inttest-token-noparams";
    let (base, state) = start_fake(
        FakeJenkins {
            queue_ready_after: 1,
            build_finishes_after: 1,
            ..Default::default()
        },
        &basic_auth(token),
    )
    .await;

    let config = config_for("inttest-noparams", &base, token);
    let client = JenkinsClient::new(&config).expect("构造客户端");
    client.trigger_build("demo", &[]).await.expect("触发构建");

    let paths = state.requests.lock().unwrap().clone();
    assert!(
        paths.iter().any(|p| p.ends_with("POST /job/demo/build")),
        "应当走 build，实际请求：{paths:?}"
    );

    cleanup("inttest-noparams");
}

/// 认证失败要能识别成 401，并给出可操作的中文提示。
#[tokio::test]
async fn unauthorized_is_reported_clearly() {
    if skip_without_keyring() {
        return;
    }
    let token = "inttest-token-401";
    let (base, _state) = start_fake(
        FakeJenkins {
            force_status: Some(401),
            ..Default::default()
        },
        &basic_auth(token),
    )
    .await;

    let config = config_for("inttest-401", &base, token);
    let client = JenkinsClient::new(&config).expect("构造客户端");
    let err = client.trigger_build("demo", &[]).await.unwrap_err();
    let message = err.user_message("dev");
    assert!(message.contains("认证失败"), "实际提示：{message}");
    assert!(message.contains("API Token"), "实际提示：{message}");

    cleanup("inttest-401");
}

/// Job 不存在时给出「找不到 Job」。
#[tokio::test]
async fn missing_job_is_reported_as_not_found() {
    if skip_without_keyring() {
        return;
    }
    let token = "inttest-token-404";
    let (base, _state) = start_fake(
        FakeJenkins {
            queue_ready_after: 1,
            build_finishes_after: 1,
            ..Default::default()
        },
        &basic_auth(token),
    )
    .await;

    let config = config_for("inttest-404", &base, token);
    let client = JenkinsClient::new(&config).expect("构造客户端");
    let err = client.trigger_build("missing-job", &[]).await.unwrap_err();
    let message = err.user_message("test");
    assert!(message.contains("找不到"), "实际提示：{message}");

    cleanup("inttest-404");
}

/// 构建失败要如实反映成 Failure，而不是被当成成功。
#[tokio::test]
async fn failed_build_reports_failure() {
    if skip_without_keyring() {
        return;
    }
    let token = "inttest-token-fail";
    let (base, _state) = start_fake(
        FakeJenkins {
            queue_ready_after: 1,
            build_finishes_after: 1,
            final_result: Some("FAILURE".to_string()),
            ..Default::default()
        },
        &basic_auth(token),
    )
    .await;

    let config = config_for("inttest-fail", &base, token);
    let client = JenkinsClient::new(&config).expect("构造客户端");
    let status = client
        .wait_for_build(
            "demo",
            42,
            Duration::from_millis(20),
            Duration::from_secs(10),
            |_| {},
        )
        .await
        .expect("查询构建状态");
    assert_eq!(status.state, BuildState::Failure);
    assert!(!status.state.is_success());
    assert_eq!(status.state.label(), "失败");

    cleanup("inttest-fail");
}

/// UNSTABLE 不能算成功，否则会把有测试失败的构建推到 test 环境。
#[tokio::test]
async fn unstable_build_is_not_success() {
    if skip_without_keyring() {
        return;
    }
    let token = "inttest-token-unstable";
    let (base, _state) = start_fake(
        FakeJenkins {
            queue_ready_after: 1,
            build_finishes_after: 1,
            final_result: Some("UNSTABLE".to_string()),
            ..Default::default()
        },
        &basic_auth(token),
    )
    .await;

    let config = config_for("inttest-unstable", &base, token);
    let client = JenkinsClient::new(&config).expect("构造客户端");
    let status = client
        .wait_for_build(
            "demo",
            42,
            Duration::from_millis(20),
            Duration::from_secs(10),
            |_| {},
        )
        .await
        .expect("查询构建状态");
    assert_eq!(status.state, BuildState::Unstable);
    assert!(!status.state.is_success());

    cleanup("inttest-unstable");
}

/// 排队一直不结束时要按超时退出，而不是永久卡住。
#[tokio::test]
async fn queue_timeout_is_reported() {
    if skip_without_keyring() {
        return;
    }
    let token = "inttest-token-timeout";
    // queue_ready_after 设得很大 => 永远停在排队状态。
    let (base, _state) = start_fake(
        FakeJenkins {
            queue_ready_after: usize::MAX,
            build_finishes_after: 1,
            ..Default::default()
        },
        &basic_auth(token),
    )
    .await;

    let mut config = config_for("inttest-timeout", &base, token);
    config.timeout_minutes = 1;
    let client = JenkinsClient::new(&config).expect("构造客户端");
    let queue = client.trigger_build("demo", &[]).await.expect("触发构建");

    // 用一个很短的超时验证超时分支。
    let err = client
        .resolve_build_number(
            &queue,
            Duration::from_millis(10),
            Duration::from_millis(120),
            || {},
        )
        .await
        .unwrap_err();
    let message = err.user_message("dev");
    assert!(message.contains("队列"), "实际提示：{message}");

    cleanup("inttest-timeout");
}

/// probe 能拿到版本号，用来确认 Jenkins 版本是否符合预期。
#[tokio::test]
async fn probe_reads_jenkins_version() {
    if skip_without_keyring() {
        return;
    }
    let token = "inttest-token-probe";
    let (base, _state) = start_fake(FakeJenkins::default(), &basic_auth(token)).await;

    let config = config_for("inttest-probe", &base, token);
    let client = JenkinsClient::new(&config).expect("构造客户端");
    let info = client.probe().await.expect("探测");
    assert_eq!(info.version.as_deref(), Some("2.528.3"));
    assert_eq!(info.mode.as_deref(), Some("NORMAL"));
    assert_eq!(info.username, "tester");

    cleanup("inttest-probe");
}

// ---- 以下用例不需要凭据管理器，默认就会执行 ----

/// 没存 token 时要在发起请求前就报错，并告诉用户去哪里补。
#[test]
fn missing_token_is_rejected_early() {
    cleanup("inttest-notoken");
    let mut config = StageConfig::new("inttest-notoken");
    config.base_url = "http://127.0.0.1:1".to_string();
    config.username = "tester".to_string();
    // JenkinsClient 没有实现 Debug，所以用 match 而不是 unwrap_err。
    match JenkinsClient::new(&config) {
        Ok(_) => panic!("没有 token 时不应该构造成功"),
        Err(err) => assert!(err.contains("还没有保存 API Token"), "实际错误：{err}"),
    }
}

/// 地址为空时也要在本地就被拦下，不必等网络超时。
#[test]
fn missing_base_url_is_rejected_early() {
    let config = StageConfig::new("inttest-nourl");
    match JenkinsClient::new(&config) {
        Ok(_) => panic!("没有地址时不应该构造成功"),
        Err(err) => assert!(err.contains("还没有配置 Jenkins 地址"), "实际错误：{err}"),
    }
}

/// 地址缺少协议头时给出明确提示。
#[test]
fn base_url_requires_scheme() {
    let mut config = StageConfig::new("inttest-noscheme");
    config.base_url = "jenkins.internal:8080".to_string();
    config.username = "tester".to_string();
    match JenkinsClient::new(&config) {
        Ok(_) => panic!("缺少协议头时不应该构造成功"),
        Err(err) => assert!(err.contains("http://"), "实际错误：{err}"),
    }
}

/// 远程 Job 列表：应当穿过文件夹递归，并跳过文件夹本身。
#[tokio::test]
async fn lists_remote_jobs_across_folders() {
    if skip_without_keyring() {
        return;
    }
    let token = "inttest-token-listjobs";
    let (base, state) = start_fake(
        FakeJenkins {
            queue_ready_after: 1,
            build_finishes_after: 1,
            ..Default::default()
        },
        &basic_auth(token),
    )
    .await;

    let config = config_for("inttest-listjobs", &base, token);
    let client = JenkinsClient::new(&config).expect("构造客户端");
    let jobs = client.list_jobs().await.expect("拉取 Job 列表");

    let names: Vec<&str> = jobs.iter().map(|j| j.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["fsk-esale", "pipe-a", "team-a/svc-1"],
        "应当包含顶层 Job 与文件夹里的 Job，且不含文件夹本身"
    );
    assert!(
        jobs.iter().all(|j| j.name != "team-a"),
        "文件夹不该被列为可添加的服务"
    );

    // 用对了 tree 参数。
    let requests = state.requests.lock().unwrap().clone();
    assert!(
        requests.iter().any(|p| p.starts_with("GET /api/json?tree=jobs")),
        "实际请求：{requests:?}"
    );

    cleanup("inttest-listjobs");
}

/// Job 名里的空格要转义，否则请求 URL 不合法。
#[test]
fn job_names_are_url_encoded() {
    let token = "inttest-token-encode";
    let config = config_for("inttest-encode", "http://127.0.0.1:1", token);
    assert!(JenkinsClient::new(&config).is_ok());
    cleanup("inttest-encode");
}

/// 参数化 Job 上用「无参触发」会被 Jenkins 以 400 Nothing is submitted 拒绝，
/// 客户端必须自动改走 buildWithParameters 重试 —— 这正是一次真实故障的原因。
#[tokio::test]
async fn parameterized_job_falls_back_to_build_with_parameters() {
    if skip_without_keyring() {
        return;
    }
    let token = "inttest-token-paramjob";
    let (base, state) = start_fake(
        FakeJenkins {
            queue_ready_after: 1,
            build_finishes_after: 1,
            ..Default::default()
        },
        &basic_auth(token),
    )
    .await;

    let config = config_for("inttest-paramjob", &base, token);
    let client = JenkinsClient::new(&config).expect("构造客户端");

    // 没有配参数，但 Job 自己有参数定义。
    let queue = client
        .trigger_build("paramjob", &[])
        .await
        .expect("应当自动重试并触发成功");
    assert!(queue.url.contains("/queue/item/1"), "队列地址：{}", queue.url);

    let requests = state.requests.lock().unwrap().clone();
    // 顺序：先试无参入口被拒，再改走 buildWithParameters。
    assert!(
        requests.iter().any(|p| p == "POST /job/paramjob/build"),
        "应当先尝试无参入口，实际请求：{requests:?}"
    );
    assert!(
        requests
            .iter()
            .any(|p| p == "POST /job/paramjob/buildWithParameters"),
        "应当回退到 buildWithParameters，实际请求：{requests:?}"
    );

    cleanup("inttest-paramjob");
}

/// 反之：Job 没有参数定义时，无参入口直接成功，不应该多做一次尝试。
#[tokio::test]
async fn plain_job_triggers_without_parameters() {
    if skip_without_keyring() {
        return;
    }
    let token = "inttest-token-plain";
    let (base, state) = start_fake(
        FakeJenkins {
            queue_ready_after: 1,
            build_finishes_after: 1,
            ..Default::default()
        },
        &basic_auth(token),
    )
    .await;

    let config = config_for("inttest-plain", &base, token);
    let client = JenkinsClient::new(&config).expect("构造客户端");

    let queue = client
        .trigger_build("demo", &[])
        .await
        .expect("应当触发成功");
    assert!(queue.url.contains("/queue/item/1"), "队列地址：{}", queue.url);

    let requests = state.requests.lock().unwrap().clone();
    assert!(
        requests.iter().any(|p| p == "POST /job/demo/build"),
        "应当走无参入口，实际请求：{requests:?}"
    );
    assert!(
        !requests.iter().any(|p| p == "POST /job/demo/buildWithParameters"),
        "无参 Job 不该再去试带参数入口，实际请求：{requests:?}"
    );

    cleanup("inttest-plain");
}


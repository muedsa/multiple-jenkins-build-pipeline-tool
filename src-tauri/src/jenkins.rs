//! Jenkins REST API 客户端。
//!
//! 兼容 Jenkins 2.528.x 的接口：
//! - `GET  /api/json`                      探测连通性与身份
//! - `GET  /crumbIssuer/api/json`          取 CSRF crumb（POST 需要）
//! - `POST /job/{name}/buildWithParameters` 或 `POST /job/{name}/build` 触发构建
//! - `GET  /queue/item/{id}/api/json`      由队列项解析出真正的构建号
//! - `GET  /job/{name}/{n}/api/json`       查询构建状态
//!
//! 队列项 -> 构建号 这一步是必须的：Jenkins 的 `queueItem` 头只给出队列地址，
//! 构建号要等排队结束后才会出现在 `executable.number` 里。

use std::sync::Arc;
use std::time::Duration;

use reqwest::cookie::Jar;
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};

use crate::config::{BuildParam, StageConfig};

/// 一次构建的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildState {
    /// 还在执行。
    Running,
    /// 执行成功。
    Success,
    /// 构建结果不健康（测试不通过等），不算成功。
    Unstable,
    /// 构建失败。
    Failure,
    /// 被中断或取消。
    Aborted,
    /// 其它未识别的结果，带上原始字符串方便排查。
    Other(String),
}

impl BuildState {
    fn from_result(result: Option<&str>) -> Self {
        match result {
            None => BuildState::Running,
            Some("SUCCESS") => BuildState::Success,
            Some("UNSTABLE") => BuildState::Unstable,
            Some("FAILURE") => BuildState::Failure,
            Some("ABORTED") => BuildState::Aborted,
            Some(other) => BuildState::Other(other.to_string()),
        }
    }

    /// 是否已经结束。
    pub fn is_finished(&self) -> bool {
        !matches!(self, BuildState::Running)
    }

    /// 是否算成功。只有 SUCCESS 才算成功，UNSTABLE 视为失败以免带着问题进 test。
    pub fn is_success(&self) -> bool {
        matches!(self, BuildState::Success)
    }

    /// 用于展示/日志的中文描述。
    pub fn label(&self) -> String {
        match self {
            BuildState::Running => "执行中".to_string(),
            BuildState::Success => "成功".to_string(),
            BuildState::Unstable => "不稳定（有测试失败）".to_string(),
            BuildState::Failure => "失败".to_string(),
            BuildState::Aborted => "已中断".to_string(),
            BuildState::Other(s) => format!("未知结果 {s}"),
        }
    }
}

/// 单次构建的状态快照。
#[derive(Debug, Clone)]
pub struct BuildStatus {
    pub build_number: u64,
    pub state: BuildState,
    /// 构建页面地址，方便直接点开看日志。
    pub url: String,
    /// 已进行时长（秒）。
    pub duration_secs: u64,
    /// 构建显示名。目前界面没用到，但保留下来方便以后展示「#12 (redeploy)」这类名字。
    #[allow(dead_code)]
    pub display_name: String,
}

/// Jenkins 服务器返回的错误，区分「没登录」「没权限」「找不到 Job」以便给出可操作提示。
#[derive(Debug)]
pub enum JenkinsError {
    /// 网络层错误。
    Transport(String),
    /// 401：用户名或 API Token 不对。
    Unauthorized,
    /// 403：账号没有权限，或 CSRF crumb 校验失败。
    Forbidden,
    /// 404：Job 不存在。
    NotFound(String),
    /// 其它 HTTP 状态码。
    Http(u16, String),
    /// 响应不是预期结构。
    Decode(String),
    /// 等待超时。
    Timeout(String),
}

impl JenkinsError {
    /// 给用户看的中文提示。
    pub fn user_message(&self, server_label: &str) -> String {
        match self {
            JenkinsError::Transport(e) => {
                format!("[{server_label}] 无法连接 Jenkins：{e}")
            }
            JenkinsError::Unauthorized => format!(
                "[{server_label}] 认证失败(401)：请检查用户名和 API Token 是否正确"
            ),
            JenkinsError::Forbidden => format!(
                "[{server_label}] 权限不足(403)：账号可能没有构建权限，或 CSRF crumb 校验失败"
            ),
            JenkinsError::NotFound(what) => {
                format!("[{server_label}] 找不到 {what}，请确认 Job 名称是否正确")
            }
            JenkinsError::Http(code, body) => {
                format!("[{server_label}] Jenkins 返回 HTTP {code}：{}", truncate(body, 200))
            }
            JenkinsError::Decode(e) => {
                format!("[{server_label}] Jenkins 响应无法解析：{e}")
            }
            JenkinsError::Timeout(e) => format!("[{server_label}] {e}"),
        }
    }
}

impl std::fmt::Display for JenkinsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

fn truncate(text: &str, max: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= max {
        return trimmed.to_string();
    }
    let head: String = trimmed.chars().take(max).collect();
    format!("{head}…")
}

/// 连到某个 Jenkins 服务器的客户端。
#[derive(Clone)]
pub struct JenkinsClient {
    client: Client,
    base_url: String,
    username: String,
    token: String,
}

impl JenkinsClient {
    /// 按环境配置构造客户端。token 从系统凭据管理器取，键为环境名。
    pub fn new(config: &StageConfig) -> Result<Self, String> {
        let base_url = config.base_url.trim().trim_end_matches('/').to_string();
        if base_url.is_empty() {
            return Err(format!("[{}] 还没有配置 Jenkins 地址", config.name));
        }
        if !base_url.starts_with("http://") && !base_url.starts_with("https://") {
            return Err(format!(
                "[{}] Jenkins 地址需要以 http:// 或 https:// 开头",
                config.name
            ));
        }
        let username = config.username.trim().to_string();
        if username.is_empty() {
            return Err(format!("[{}] 还没有填写用户名", config.name));
        }
        let token = crate::secret::load_token(&config.name).ok_or_else(|| {
            format!(
                "[{}] 还没有保存 API Token，请到「设置」里填写",
                config.name
            )
        })?;

        let mut builder = Client::builder()
            .user_agent(concat!(
                "multiple-jenkins-build-pipeline-tool/",
                env!("CARGO_PKG_VERSION")
            ))
            .timeout(Duration::from_secs(30))
            // 打开 cookie 支持：crumb 与登录会话都要靠 cookie 延续。
            .cookie_provider(Arc::new(Jar::default()));
        if config.insecure_tls {
            builder = builder
                .danger_accept_invalid_certs(true)
                .danger_accept_invalid_hostnames(true);
        }
        let client = builder
            .build()
            .map_err(|e| format!("创建 HTTP 客户端失败：{e}"))?;

        Ok(Self {
            client,
            base_url,
            username,
            token,
        })
    }

    fn url(&self, path: &str) -> String {
        format!("{}/{}", self.base_url, path.trim_start_matches('/'))
    }

    /// Job 名里可能带 `/`（文件夹型 Job），逐段转义。
    fn job_path(job: &str) -> String {
        job.split('/')
            .filter(|seg| !seg.is_empty())
            .map(|seg| {
                format!(
                    "job/{}",
                    urlencoding_encode(seg)
                )
            })
            .collect::<Vec<_>>()
            .join("/")
    }

    /// 给人看的构建页面地址，用于界面上的「日志」链接。
    ///
    /// Jenkins 的网页地址必须带上 `job` 段：
    /// `/job/fsk-esale/3249`，文件夹型 Job 是 `/job/team/job/app/12`。
    /// 不能简单用 `地址/Job名/构建号` 拼，那样会 404。
    pub fn build_page_url(&self, job: &str, build_number: u64) -> String {
        self.url(&format!("{}/{}", Self::job_path(job), build_number))
    }

    /// 拉取这台 Jenkins 上所有可构建的 Job，供界面搜索添加。
    ///
    /// 用 `tree=jobs[name,fullName,url,_class]` 一次拿到全量列表 —— 包括文件夹
    /// 里的 Job。返回的名字是带文件夹层级的完整名（如 `team-a/order-service`），
    /// 正好就是触发时要用的 Job 名。
    ///
    /// 只保留可以直接触发的 Job 类型：文件夹和多分支这类「容器」本身不是可构建项，
    /// 列出来只会让人误选。
    pub async fn list_jobs(&self) -> Result<Vec<RemoteJob>, JenkinsError> {
        let response = self
            .client
            .get(self.url("api/json?tree=jobs[name,fullName,url,_class]"))
            .basic_auth(&self.username, Some(&self.token))
            .send()
            .await
            .map_err(|e| JenkinsError::Transport(e.to_string()))?;
        let root: JobTree = self.handle(response, || "Jenkins 首页".to_string()).await?;

        let mut out = Vec::new();
        collect_buildable_jobs(&root.jobs, None, &mut out);
        // 按名字排序，界面上浏览更直观。
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out.dedup_by(|a, b| a.name == b.name);
        Ok(out)
    }

    /// 统一处理响应状态码，把 HTTP 错误翻译成 [`JenkinsError`]。
    async fn handle<T: for<'de> Deserialize<'de>>(
        &self,
        response: reqwest::Response,
        not_found: impl FnOnce() -> String,
    ) -> Result<T, JenkinsError> {
        let status = response.status();
        if status.is_success() {
            return response
                .json::<T>()
                .await
                .map_err(|e| JenkinsError::Decode(e.to_string()));
        }
        match status {
            StatusCode::UNAUTHORIZED => Err(JenkinsError::Unauthorized),
            StatusCode::FORBIDDEN => Err(JenkinsError::Forbidden),
            StatusCode::NOT_FOUND => Err(JenkinsError::NotFound(not_found())),
            other => {
                let code = other.as_u16();
                let body = response.text().await.unwrap_or_default();
                Err(JenkinsError::Http(code, body))
            }
        }
    }

    /// 探测连通性并返回 Jenkins 版本、当前登录身份。
    pub async fn probe(&self) -> Result<ProbeInfo, JenkinsError> {
        let response = self
            .client
            .get(self.url("api/json?tree=mode,nodeDescription,version"))
            .basic_auth(&self.username, Some(&self.token))
            .send()
            .await
            .map_err(|e| JenkinsError::Transport(e.to_string()))?;
        let info: ProbeResponse = self.handle(response, || "Jenkins 首页".to_string()).await?;
        Ok(ProbeInfo {
            version: info.version,
            mode: info.mode,
            username: self.username.clone(),
        })
    }

    /// 获取 CSRF crumb。有些 Jenkins 关闭了该保护，此时返回 `None`。
    async fn fetch_crumb(&self) -> Option<(String, String)> {
        #[derive(Deserialize)]
        struct Crumb {
            crumb: String,
            #[serde(rename = "crumbRequestField")]
            field: String,
        }
        let response = self
            .client
            .get(self.url("crumbIssuer/api/json"))
            .basic_auth(&self.username, Some(&self.token))
            .send()
            .await
            .ok()?;
        if !response.status().is_success() {
            return None;
        }
        let crumb = response.json::<Crumb>().await.ok()?;
        Some((crumb.field, crumb.crumb))
    }

    /// 把 Jenkins 给的地址补成绝对 URL。
    ///
    /// `Location` 响应头常常是 `/queue/item/12/` 这样的相对路径，
    /// 直接拿去请求会失败，所以这里统一按 base URL 补全。
    fn absolutize(&self, location: &str) -> String {
        let trimmed = location.trim();
        let joined = if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
            trimmed.to_string()
        } else if trimmed.starts_with('/') {
            format!("{}{}", self.base_url, trimmed)
        } else {
            format!("{}/{}", self.base_url, trimmed)
        };
        // 去掉末尾多余的 `/`，这样拼查询串时不会出现 `/1/?tree=` 这种地址。
        joined.trim_end_matches('/').to_string()
    }

    /// 触发一次构建。
    ///
    /// 返回队列项地址（`Location` 响应头）。构建号需要另外解析，见 [`Self::resolve_build_number`]。
    ///
    /// 端点选择不能只看「我们有没有配参数」：Job 自己可能定义了参数，
    /// 这时打 `/build` 会被 Jenkins 以 400 `Nothing is submitted` 拒绝。
    /// 所以先试无参入口，被拒后自动改走 `buildWithParameters`（不传参数值时用 Job 默认值）。
    pub async fn trigger_build(
        &self,
        job: &str,
        params: &[BuildParam],
    ) -> Result<QueueRef, JenkinsError> {
        let form: Vec<(String, String)> = params
            .iter()
            .map(|p| (p.key.clone(), p.value.clone()))
            .collect();

        let attempts: [&str; 2] = if form.is_empty() {
            // 没配参数：先按「Job 不需要参数」处理，被拒（参数化 Job 会回
            // 400 Nothing is submitted）再改走带参数入口，用 Job 自身的默认值。
            ["build", "buildWithParameters"]
        } else {
            // 配了参数：只该走带参数入口。
            ["buildWithParameters", "buildWithParameters"]
        };

        let mut last: Option<JenkinsError> = None;
        let mut seen = Vec::new();
        for endpoint in attempts {
            if seen.contains(&endpoint) {
                continue;
            }
            seen.push(endpoint);
            match self.post_build(job, endpoint, &form).await {
                Ok(queue) => return Ok(queue),
                Err(err) => {
                    // 只有「参数化 Job 被无参触发」这一种情况才值得换端点重试。
                    // 认证失败、没权限、Job 不存在等错误换端点也没用，直接返回。
                    let retryable = matches!(
                        &err,
                        JenkinsError::Http(400, body)
                            if body.contains("Nothing is submitted")
                    );
                    last = Some(err);
                    if !retryable {
                        break;
                    }
                }
            }
        }
        Err(last.unwrap_or(JenkinsError::Transport("触发构建失败".to_string())))
    }

    /// 向指定端点发起一次触发请求。
    async fn post_build(
        &self,
        job: &str,
        endpoint: &str,
        form: &[(String, String)],
    ) -> Result<QueueRef, JenkinsError> {
        let url = self.url(&format!("{}/{}", Self::job_path(job), endpoint));

        let mut request = self
            .client
            .post(&url)
            .basic_auth(&self.username, Some(&self.token));
        if let Some((field, crumb)) = self.fetch_crumb().await {
            request = request.header(field, crumb);
        }
        if !form.is_empty() {
            request = request.form(form);
        }

        let response = request
            .send()
            .await
            .map_err(|e| JenkinsError::Transport(e.to_string()))?;

        let status = response.status();
        // 触发成功时 Jenkins 返回 201 并带 Location 头；旧版本可能返回 200。
        if status.is_success() {
            if let Some(location) = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|v| v.to_str().ok())
            {
                return Ok(QueueRef {
                    url: self.absolutize(location),
                });
            }
            // 没有 Location 就退回队列首页，后面从队列里按 Job 名找。
            return Ok(QueueRef {
                url: self.url("queue"),
            });
        }

        match status {
            StatusCode::UNAUTHORIZED => Err(JenkinsError::Unauthorized),
            StatusCode::FORBIDDEN => Err(JenkinsError::Forbidden),
            StatusCode::NOT_FOUND => Err(JenkinsError::NotFound(format!("Job「{job}」"))),
            other => {
                let code = other.as_u16();
                let body = response.text().await.unwrap_or_default();
                Err(JenkinsError::Http(code, body))
            }
        }
    }

    /// 由队列项解析出构建号。排队期间 Jenkins 还没有分配构建号，这里会一直等到分配为止。
    pub async fn resolve_build_number(
        &self,
        queue: &QueueRef,
        poll_interval: Duration,
        timeout: Duration,
        // 每次探测后回调，方便前端展示「排队中」。
        mut on_progress: impl FnMut(),
    ) -> Result<Option<u64>, JenkinsError> {
        let deadline = tokio::time::Instant::now() + timeout;
        // 队列项地址来自 Location 头，取状态还要再拼上 api/json。
        let url = format!("{}/api/json", queue.url.trim_end_matches('/'));
        loop {
            let response = self
                .client
                .get(&url)
                .query(&[("tree", "executable[number,url],why,cancelled")])
                .basic_auth(&self.username, Some(&self.token))
                .send()
                .await
                .map_err(|e| JenkinsError::Transport(e.to_string()))?;
            let item: QueueItem = self
                .handle(response, || "队列项".to_string())
                .await?;

            if item.cancelled.unwrap_or(false) {
                return Ok(None);
            }
            if let Some(exec) = item.executable {
                return Ok(Some(exec.number));
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(JenkinsError::Timeout(format!(
                    "等待构建进入执行阶段超过 {} 分钟仍未分配构建号（队列原因：{}）",
                    timeout.as_secs() / 60,
                    item.why.unwrap_or_else(|| "未知".to_string())
                )));
            }
            on_progress();
            tokio::time::sleep(poll_interval).await;
        }
    }

    /// 查询某个构建的当前状态。
    pub async fn build_status(
        &self,
        job: &str,
        build_number: u64,
    ) -> Result<BuildStatus, JenkinsError> {
        let path = format!(
            "{}/{}/api/json?tree=number,building,result,duration,url,displayName",
            Self::job_path(job),
            build_number
        );
        let response = self
            .client
            .get(self.url(&path))
            .basic_auth(&self.username, Some(&self.token))
            .send()
            .await
            .map_err(|e| JenkinsError::Transport(e.to_string()))?;
        let info: BuildInfo = self
            .handle(response, || format!("构建 {job}#{build_number}"))
            .await?;

        // building=true 时 result 仍为 null。
        let state = if info.building.unwrap_or(false) {
            BuildState::Running
        } else {
            BuildState::from_result(info.result.as_deref())
        };
        Ok(BuildStatus {
            build_number: info.number.unwrap_or(build_number),
            state,
            url: info.url.unwrap_or_default(),
            duration_secs: info.duration.unwrap_or(0) / 1000,
            display_name: info.display_name.unwrap_or_default(),
        })
    }

    /// 等待构建结束，期间按 `poll_interval` 轮询。
    pub async fn wait_for_build(
        &self,
        job: &str,
        build_number: u64,
        poll_interval: Duration,
        timeout: Duration,
        mut on_progress: impl FnMut(&BuildStatus),
    ) -> Result<BuildStatus, JenkinsError> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let status = self.build_status(job, build_number).await?;
            if status.state.is_finished() {
                return Ok(status);
            }
            on_progress(&status);
            if tokio::time::Instant::now() >= deadline {
                return Err(JenkinsError::Timeout(format!(
                    "构建 {job}#{build_number} 超过 {} 分钟仍未结束",
                    timeout.as_secs() / 60
                )));
            }
            tokio::time::sleep(poll_interval).await;
        }
    }
}

/// 触发构建后拿到的队列引用。
#[derive(Debug, Clone)]
pub struct QueueRef {
    pub url: String,
}

/// 从 Jenkins 拉到的可构建 Job，用于界面搜索添加。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteJob {
    /// 带文件夹层级的完整名，也就是触发时该填的 Job 名。
    pub name: String,
    /// 展示用的末端名字。
    pub display_name: String,
    /// Job 在 Jenkins 上的页面地址。
    pub url: String,
}

// ---- Jenkins `tree=jobs[...]` 的响应结构 ----

#[derive(Debug, Deserialize)]
struct JobTree {
    #[serde(default)]
    jobs: Vec<JobNode>,
}

#[derive(Debug, Deserialize)]
struct JobNode {
    #[serde(default)]
    name: Option<String>,
    /// 文件夹里的 Job 会带上完整路径；老版本可能不返回，这时靠父级拼。
    #[serde(default, rename = "fullName")]
    full_name: Option<String>,
    #[serde(default)]
    url: Option<String>,
    /// Jenkins 的类名，用来区分叶子 Job 与文件夹/多分支容器。
    #[serde(default, rename = "_class")]
    class: Option<String>,
    #[serde(default)]
    jobs: Option<Vec<JobNode>>,
}

/// 递归收集可构建的 Job。
///
/// `parent` 是父级完整名，用于在 Jenkins 没返回 `fullName` 时自己拼层级。
fn collect_buildable_jobs(nodes: &[JobNode], parent: Option<&str>, out: &mut Vec<RemoteJob>) {
    for node in nodes {
        let Some(name) = node.name.as_deref().map(str::trim).filter(|n| !n.is_empty()) else {
            continue;
        };
        // 优先用 Jenkins 给的 fullName，没有就按父级拼。
        let full = node
            .full_name
            .as_deref()
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| match parent {
                Some(p) if !p.is_empty() => format!("{p}/{name}"),
                _ => name.to_string(),
            });

        match node.class.as_deref() {
            // 容器类型：继续往下找，它们自己不作为可选项。
            Some(class) if is_container_class(class) => {
                if let Some(children) = &node.jobs {
                    collect_buildable_jobs(children, Some(&full), out);
                }
            }
            // 明确是可构建的叶子类型。
            Some(class) if is_buildable_class(class) => out.push(RemoteJob {
                display_name: name.to_string(),
                url: node.url.clone().unwrap_or_default(),
                name: full,
            }),
            // 认不出类型时保守处理：有子项就当文件夹递归，没子项就当作可构建 Job。
            Some(_) | None => match &node.jobs {
                Some(children) if !children.is_empty() => {
                    collect_buildable_jobs(children, Some(&full), out)
                }
                _ => out.push(RemoteJob {
                    display_name: name.to_string(),
                    url: node.url.clone().unwrap_or_default(),
                    name: full,
                }),
            },
        }
    }
}

/// 文件夹、多分支流水线这类「容器」——它们本身不能直接触发构建。
fn is_container_class(class: &str) -> bool {
    matches!(
        class,
        "com.cloudbees.hudson.plugins.folder.Folder"
            | "jenkins.branch.OrganizationFolder"
            | "org.jenkinsci.plugins.workflow.multibranch.WorkflowMultiBranchProject"
            | "hudson.model.View"
    )
}

/// 明确可以触发构建的 Job 类型。
fn is_buildable_class(class: &str) -> bool {
    matches!(
        class,
        "hudson.model.FreeStyleProject"
            | "org.jenkinsci.plugins.workflow.job.WorkflowJob"
            | "hudson.model.Project"
            | "hudson.maven.MavenModuleSet"
            | "hudson.matrix.MatrixProject"
    )
}

/// `probe` 的结果。
#[derive(Debug, Clone)]
pub struct ProbeInfo {
    pub version: Option<String>,
    pub mode: Option<String>,
    /// 当前发起请求的 Jenkins 用户名。
    #[allow(dead_code)]
    pub username: String,
}

// ---- Jenkins 响应体的反序列化结构 ----

#[derive(Debug, Deserialize)]
struct ProbeResponse {
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    mode: Option<String>,
}

#[derive(Debug, Deserialize)]
struct QueueItem {
    #[serde(default)]
    executable: Option<QueueExecutable>,
    #[serde(default)]
    why: Option<String>,
    #[serde(default)]
    cancelled: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct QueueExecutable {
    #[serde(default)]
    number: u64,
}

#[derive(Debug, Deserialize)]
struct BuildInfo {
    #[serde(default)]
    number: Option<u64>,
    #[serde(default)]
    building: Option<bool>,
    #[serde(default)]
    result: Option<String>,
    #[serde(default)]
    duration: Option<u64>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default, rename = "displayName")]
    display_name: Option<String>,
}

/// 对 Job 名里的单段做 URL 转义。
///
/// 只保留 URL 里安全的字符，其余按 UTF-8 百分号编码，
/// 这样带空格或中文的 Job 名也能正确请求。
fn urlencoding_encode(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    for byte in segment.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_path_encodes_each_segment() {
        assert_eq!(JenkinsClient::job_path("my-service"), "job/my-service");
        // 文件夹型 Job 用 / 分隔，每段单独转义。
        assert_eq!(
            JenkinsClient::job_path("team-a/my service"),
            "job/team-a/job/my%20service"
        );
    }

    /// 构建页面地址必须带 `job` 段，否则浏览器打开会 404。
    #[test]
    fn build_page_url_includes_job_segment() {
        // 只是拼地址，不发起请求；这里直接构造客户端以避免依赖真实凭据。
        let mut config = StageConfig::new("dev");
        config.base_url = "https://dev-jenkins.fskops.com/".to_string();
        config.username = "tester".to_string();
        let client = JenkinsClient {
            client: Client::new(),
            base_url: config.base_url.trim_end_matches('/').to_string(),
            username: config.username.clone(),
            token: String::new(),
        };

        assert_eq!(
            client.build_page_url("fsk-esale", 3249),
            "https://dev-jenkins.fskops.com/job/fsk-esale/3249"
        );
        // 文件夹型 Job：每一级都要有 job 段。
        assert_eq!(
            client.build_page_url("team-a/my-service", 12),
            "https://dev-jenkins.fskops.com/job/team-a/job/my-service/12"
        );
    }

    /// 远程 Job 列表要能穿过文件夹递归，并且排除文件夹本身。
    #[test]
    fn collects_buildable_jobs_recursively() {
        // 模拟 /api/json?tree=jobs[name,fullName,url,_class] 的返回。
        let raw = r####"{
          "jobs": [
            { "_class": "hudson.model.FreeStyleProject", "name": "fsk-esale",
              "fullName": "fsk-esale", "url": "https://j/job/fsk-esale/" },
            { "_class": "org.jenkinsci.plugins.workflow.job.WorkflowJob", "name": "pipe-a",
              "fullName": "pipe-a", "url": "https://j/job/pipe-a/" },
            { "_class": "com.cloudbees.hudson.plugins.folder.Folder", "name": "team-a",
              "fullName": "team-a", "url": "https://j/job/team-a/",
              "jobs": [
                { "_class": "hudson.model.FreeStyleProject", "name": "svc-1",
                  "fullName": "team-a/svc-1", "url": "https://j/job/team-a/job/svc-1/" },
                { "_class": "com.cloudbees.hudson.plugins.folder.Folder", "name": "inner",
                  "fullName": "team-a/inner", "url": "https://j/job/team-a/job/inner/",
                  "jobs": [
                    { "_class": "hudson.model.FreeStyleProject", "name": "svc-2",
                      "fullName": "team-a/inner/svc-2", "url": "https://j/job/team-a/job/inner/job/svc-2/" }
                  ] }
              ] },
            { "_class": "org.jenkinsci.plugins.workflow.multibranch.WorkflowMultiBranchProject",
              "name": "multi", "fullName": "multi", "url": "https://j/job/multi/", "jobs": [] }
          ]
        }"####;

        let root: JobTree = serde_json::from_str(raw).expect("解析 jobs 树");
        let mut out = Vec::new();
        collect_buildable_jobs(&root.jobs, None, &mut out);
        out.sort_by(|a, b| a.name.cmp(&b.name));

        let names: Vec<&str> = out.iter().map(|j| j.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "fsk-esale",
                "pipe-a",
                "team-a/inner/svc-2",
                "team-a/svc-1"
            ],
            "应当递归收集叶子 Job，并排除文件夹/多分支容器本身"
        );
        // 展示名用末端名字，完整名带层级。
        let svc2 = out.iter().find(|j| j.name == "team-a/inner/svc-2").unwrap();
        assert_eq!(svc2.display_name, "svc-2");
    }

    /// Jenkins 没返回 fullName 时要能靠父级自己拼出层级路径。
    #[test]
    fn falls_back_to_building_full_name_from_parent() {
        let raw = r####"{
          "jobs": [
            { "_class": "com.cloudbees.hudson.plugins.folder.Folder", "name": "team-a",
              "jobs": [
                { "_class": "hudson.model.FreeStyleProject", "name": "svc-1" }
              ] }
          ]
        }"####;
        let root: JobTree = serde_json::from_str(raw).expect("解析");
        let mut out = Vec::new();
        collect_buildable_jobs(&root.jobs, None, &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "team-a/svc-1");
    }

    #[test]
    fn build_state_maps_jenkins_results() {        assert_eq!(BuildState::from_result(None), BuildState::Running);
        assert!(BuildState::from_result(Some("SUCCESS")).is_success());
        assert!(!BuildState::from_result(Some("UNSTABLE")).is_success());
        assert_eq!(
            BuildState::from_result(Some("FAILURE")),
            BuildState::Failure
        );
    }
}

//! Jenkins 构建管道工具 —— Tauri 后端入口。
//!
//! 命令分成四组：
//! - 配置：读取/保存服务器、服务清单、通知模板
//! - 凭据：把 API Token 存进系统凭据管理器
//! - 诊断：测试两个 Jenkins 的连通性与认证
//! - 流水线：启动、停止、查询进度

// 模块对外可见，方便集成测试直接使用（tests/ 目录下的测试属于独立 crate）。
pub mod config;
pub mod jenkins;
pub mod notify;
pub mod queue;
pub mod secret;
pub mod tray;

use std::sync::Arc;

use serde::Serialize;
use tauri::Manager;

use config::{AppConfig, BuildParam, ConfigStore, Service, StageConfig, TemplateConfig};
use jenkins::{JenkinsClient, RemoteJob};
use notify::{humanize_duration, render_template, TemplateVars};
use queue::QueueRegistry;

/// 全局状态：配置 + 发布队列。
struct AppState {
    config: ConfigStore,
    queue: Arc<QueueRegistry>,
}

// ---- 配置相关命令 ----

/// 读取当前配置。token 不会出现在返回值里，只有 `hasToken` 表示是否已保存。
#[tauri::command]
fn get_config(state: tauri::State<'_, AppState>) -> Result<AppConfig, String> {
    state.config.get()
}

/// 整体覆盖保存配置。
#[tauri::command]
fn save_config(
    state: tauri::State<'_, AppState>,
    config: AppConfig,
) -> Result<AppConfig, String> {
    state.config.replace(config)
}

/// 新增或更新一个流水线环境。`originalName` 非空时表示改名。
///
/// 环境名就是凭据管理器里的键；改名时会把 token 一并挪过去，
/// 免得用户以为 token 丢了。
#[tauri::command]
fn save_stage(
    state: tauri::State<'_, AppState>,
    stage: StageConfig,
    original_name: Option<String>,
) -> Result<AppConfig, String> {
    let new_name = stage.name.trim().to_string();
    if new_name.is_empty() {
        return Err("环境名不能为空".to_string());
    }
    let old_name = original_name.unwrap_or_else(|| new_name.clone());

    // 改名时先搬迁凭据：存在旧名字下的 token 挪到新名字。
    if old_name != new_name && secret::has_token(&old_name) {
        secret::rename_token(&old_name, &new_name)?;
    }

    state.config.update(|config| {
        match config.stages.iter_mut().find(|s| s.name == old_name) {
            Some(existing) => *existing = stage,
            // 找不到就当作新增（例如 UI 直接提交了一条新环境）。
            None => config.stages.push(stage),
        }
    })
}

/// 删除一个环境。
#[tauri::command]
fn delete_stage(
    state: tauri::State<'_, AppState>,
    name: String,
) -> Result<AppConfig, String> {
    state.config.update(|config| {
        config.stages.retain(|s| s.name != name);
    })
}

/// 调整环境顺序（数组顺序即执行顺序）。
#[tauri::command]
fn reorder_stages(
    state: tauri::State<'_, AppState>,
    names: Vec<String>,
) -> Result<AppConfig, String> {
    state.config.update(|config| {
        let mut ordered = Vec::with_capacity(config.stages.len());
        // 先按给定的名字顺序排，再补上没提到的（避免误删）。
        for name in &names {
            if let Some(pos) = config.stages.iter().position(|s| &s.name == name) {
                ordered.push(config.stages.remove(pos));
            }
        }
        ordered.append(&mut config.stages);
        config.stages = ordered;
    })
}

/// 保存 API Token 到系统凭据管理器（键为环境名）。
#[tauri::command]
fn save_token(stage: String, token: String) -> Result<(), String> {
    secret::save_token(&stage, &token)
}

/// 删除已保存的 token。
#[tauri::command]
fn clear_token(stage: String) -> Result<(), String> {
    secret::clear_token(&stage)
}

/// 查询每个环境是否都已经存好 token。
#[tauri::command]
fn token_status(state: tauri::State<'_, AppState>) -> Result<Vec<StageToken>, String> {
    let config = state.config.get()?;
    Ok(config
        .stages
        .iter()
        .map(|s| StageToken {
            name: s.name.clone(),
            has_token: secret::has_token(&s.name),
        })
        .collect())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct StageToken {
    name: String,
    has_token: bool,
}

// ---- 服务清单 ----

/// 新增或更新一个服务。`originalName` 非空时表示重命名。
#[tauri::command]
fn save_service(
    state: tauri::State<'_, AppState>,
    service: Service,
    original_name: Option<String>,
) -> Result<AppConfig, String> {
    let name = service.name.trim().to_string();
    if name.is_empty() {
        return Err("服务名不能为空".to_string());
    }
    state.config.update(|config| {
        let old = original_name.as_deref().unwrap_or(&name);
        // 重命名或改名后如果和已有服务撞名，直接覆盖那一条，避免出现重复。
        config.services.retain(|s| s.name != name && s.name != old);
        config.services.push(Service {
            name: name.clone(),
            favorite: service.favorite,
            params: service.params.clone(),
        });
        // 排序：常用在前，其次按名称，方便快速勾选。
        config.services.sort_by(|a, b| {
            b.favorite
                .cmp(&a.favorite)
                .then_with(|| a.name.cmp(&b.name))
        });
        config.last_selection = config
            .last_selection
            .iter()
            .map(|s| if s == old { name.clone() } else { s.clone() })
            .collect();
    })
}

/// 删除一个服务。
#[tauri::command]
fn delete_service(
    state: tauri::State<'_, AppState>,
    name: String,
) -> Result<AppConfig, String> {
    state.config.update(|config| {
        config.services.retain(|s| s.name != name);
        config.last_selection.retain(|s| s != &name);
    })
}

/// 切换「常用」标记。
#[tauri::command]
fn toggle_favorite(
    state: tauri::State<'_, AppState>,
    name: String,
) -> Result<AppConfig, String> {
    state.config.update(|config| {
        if let Some(service) = config.services.iter_mut().find(|s| s.name == name) {
            service.favorite = !service.favorite;
        }
        config.services.sort_by(|a, b| {
            b.favorite
                .cmp(&a.favorite)
                .then_with(|| a.name.cmp(&b.name))
        });
    })
}

/// 批量新增服务（方便从列表一次性粘贴导入）。
#[tauri::command]
fn add_services(
    state: tauri::State<'_, AppState>,
    names: Vec<String>,
) -> Result<AppConfig, String> {
    let cleaned: Vec<String> = names
        .iter()
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
        .collect();
    if cleaned.is_empty() {
        return Err("没有可添加的服务名".to_string());
    }
    state.config.update(|config| {
        for name in cleaned {
            if config.services.iter().any(|s| s.name == name) {
                continue;
            }
            config.services.push(Service {
                name,
                favorite: false,
                params: Vec::new(),
            });
        }
        config.services.sort_by(|a, b| {
            b.favorite
                .cmp(&a.favorite)
                .then_with(|| a.name.cmp(&b.name))
        });
    })
}

/// 记住本次勾选的服务，下次启动时自动回填。
#[tauri::command]
fn save_selection(
    state: tauri::State<'_, AppState>,
    names: Vec<String>,
) -> Result<(), String> {
    state.config.update(|config| {
        config.last_selection = names;
    })?;
    Ok(())
}

// ---- 通知模板 ----

/// 保存通知文案模板，并返回按示例数据渲染出的预览。
#[tauri::command]
fn save_template(
    state: tauri::State<'_, AppState>,
    template: TemplateConfig,
) -> Result<AppConfig, String> {
    state.config.update(|config| config.template = template)
}

/// 用示例数据渲染模板，供设置页实时预览。
#[tauri::command]
fn preview_template(template: TemplateConfig) -> PreviewContent {
    let vars = sample_vars();
    PreviewContent {
        success_title: render_template(&template.title, &vars),
        success_body: render_template(&template.body, &vars),
        failure_title: render_template(&template.failure_title, &vars),
        failure_body: render_template(&template.failure_body, &vars),
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PreviewContent {
    success_title: String,
    success_body: String,
    failure_title: String,
    failure_body: String,
}

/// 预览用的示例变量。数字刻意取得直观，方便一眼看出文案效果。
fn sample_vars() -> TemplateVars {
    TemplateVars {
        service_count: 3,
        success_count: 2,
        failed_count: 1,
        success_list: "order-service, user-service".to_string(),
        failed_list: "pay-service".to_string(),
        service_list: "order-service, user-service, pay-service".to_string(),
        duration: humanize_duration(372),
        status: "失败".to_string(),
        finished_at: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        stage_chain: "dev → sit → prod".to_string(),
        stage_count: 3,
        service_name: "pay-service".to_string(),
        reached_stage: "sit".to_string(),
        failed_stage: "sit".to_string(),
    }
}

// ---- 连通性诊断 ----

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProbeResult {
    ok: bool,
    message: String,
    /// Jenkins 版本，便于确认是否为 2.528.x。
    version: Option<String>,
    mode: Option<String>,
}

/// 测试某个 Jenkins 环境是否可用（地址 + 账号 + token）。
#[tauri::command]
async fn test_connection(stage: StageConfig) -> Result<ProbeResult, String> {
    let client = match JenkinsClient::new(&stage) {
        Ok(client) => client,
        Err(err) => {
            return Ok(ProbeResult {
                ok: false,
                message: err,
                version: None,
                mode: None,
            })
        }
    };
    match client.probe().await {
        Ok(info) => Ok(ProbeResult {
            ok: true,
            message: format!(
                "连接成功{}",
                info.version
                    .as_deref()
                    .map(|v| format!("，Jenkins 版本 {v}"))
                    .unwrap_or_default()
            ),
            version: info.version,
            mode: info.mode,
        }),
        Err(err) => Ok(ProbeResult {
            ok: false,
            message: err.user_message(&stage.name),
            version: None,
            mode: None,
        }),
    }
}

/// 从某台 Jenkins 拉取所有可构建的 Job，供界面搜索添加服务。
///
/// `stage` 指定查哪个环境；不传就用流水线里的第一个环境
/// （服务清单本来就是按「各环境同名 Job」的约定维护的）。
#[tauri::command]
async fn list_remote_jobs(
    state: tauri::State<'_, AppState>,
    stage: Option<String>,
) -> Result<Vec<RemoteJob>, String> {
    let config = state.config.get()?;
    let target = match stage.as_deref() {
        Some(name) => config
            .stage(name)
            .cloned()
            .ok_or_else(|| format!("找不到环境「{name}」"))?,
        None => config
            .stages
            .first()
            .cloned()
            .ok_or_else(|| "还没有配置任何流水线环境，请先到「设置」里添加".to_string())?,
    };
    if !target.is_ready() {
        return Err(format!(
            "[{}] 还没有配置好地址和用户名，请先到「设置」里填好",
            target.name
        ));
    }
    let client = JenkinsClient::new(&target)?;
    client
        .list_jobs()
        .await
        .map_err(|e| e.user_message(&target.name))
}

// ---- 流水线 ----

/// 启动发布：所选服务并行开始，各自沿环境链逐级发布。
/// 把选中的服务加入发布队列。队列内全部并行执行，可逐个取消。
#[tauri::command]
fn enqueue_tasks(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    services: Vec<String>,
) -> Result<Vec<String>, String> {
    let config = state.config.get()?;
    // 记住本次勾选，下次打开还在。
    let _ = state.config.update(|c| c.last_selection = services.clone());
    queue::enqueue(app, state.queue.clone(), config, services)
}

/// 取消单个任务，其他任务不受影响。
#[tauri::command]
fn cancel_task(state: tauri::State<'_, AppState>, task_id: String) -> Result<bool, String> {
    Ok(state.queue.cancel(&task_id))
}

/// 读取队列当前快照（页面刚打开时用它补齐状态）。
#[tauri::command]
fn queue_status(state: tauri::State<'_, AppState>) -> queue::QueueSnapshot {
    state.queue.snapshot()
}

/// 清空已经结束的任务。
#[tauri::command]
fn clear_finished_tasks(state: tauri::State<'_, AppState>) -> Result<usize, String> {
    Ok(state.queue.clear_finished())
}

/// 手动重发某次任务的通知（用户可能错过了弹窗）。
#[tauri::command]
fn resend_notification(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    task_id: String,
) -> Result<(), String> {
    let task = queue::task(&state.queue, &task_id)
        .ok_or_else(|| format!("找不到任务 {task_id}"))?;
    let content = task
        .notification
        .ok_or_else(|| "这个任务还没有结束，暂时不能重发通知".to_string())?;
    notify::send_system_notification(&app, &content)
}

/// 前端可以直接把模板和变量丢过来渲染，便于「立即预览」。
#[tauri::command]
fn render_message(template: String, vars: Option<TemplateVars>) -> String {
    render_template(&template, &vars.unwrap_or_else(sample_vars))
}

/// 让前端知道有哪些变量可用。
#[tauri::command]
fn template_variables() -> Vec<TemplateVariable> {
    vec![
        TemplateVariable::new("serviceCount", "本次发布的服务总数"),
        TemplateVariable::new("successCount", "成功的服务数量"),
        TemplateVariable::new("failedCount", "失败的服务数量"),
        TemplateVariable::new("successList", "成功服务名，逗号分隔"),
        TemplateVariable::new("failedList", "失败服务名，逗号分隔"),
        TemplateVariable::new("serviceList", "全部服务名，逗号分隔"),
        TemplateVariable::new("duration", "总耗时，如 3分12秒"),
        TemplateVariable::new("status", "整体结论：成功 / 失败"),
        TemplateVariable::new("finishedAt", "完成时刻"),
        TemplateVariable::new("stageChain", "环境链路，如 dev → sit → prod"),
        TemplateVariable::new("stageCount", "环境个数"),
    ]
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TemplateVariable {
    key: String,
    description: String,
}

impl TemplateVariable {
    fn new(key: &str, description: &str) -> Self {
        Self {
            key: key.to_string(),
            description: description.to_string(),
        }
    }
}

// ---- 参数校验 ----

/// 校验一组构建参数，返回第一个不合法的参数说明。
#[tauri::command]
fn validate_params(params: Vec<BuildParam>) -> Result<(), String> {
    for param in params {
        if param.key.trim().is_empty() {
            return Err("参数名不能为空".to_string());
        }
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default();

    // 单例运行：第二个进程把命令行参数交给第一个进程后自己退出，这里只负责把主窗口唤回前台。
    //
    // 这个插件必须最先注册：插件按注册顺序初始化，而配置里的窗口是在所有插件初始化之后
    // 才创建的（见 tauri app.rs 的 setup），注册晚了第二个实例会先把窗口弹出来再退出。
    // 移动端不支持，依赖已经限定在桌面三个平台，这里用 #[cfg(desktop)] 对应上。
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, args, cwd| {
        eprintln!(
            "[single-instance] 程序已在运行，第二个实例已退出；参数 {args:?}，工作目录 {cwd}"
        );
        tray::show_main_window(app);
    }));

    builder
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            // 配置放在应用数据目录，随用户走，不会和程序目录混在一起。
            let dir = app
                .path()
                .app_data_dir()
                .map_err(|e| format!("无法定位应用数据目录：{e}"))?;
            std::fs::create_dir_all(&dir)
                .map_err(|e| format!("创建应用数据目录失败：{e}"))?;
            app.manage(AppState {
                config: ConfigStore::load(&dir),
                queue: Arc::new(QueueRegistry::default()),
            });
            // 托盘建不起来不致命（只是少了个入口），记一条日志继续跑。
            if let Err(e) = tray::setup(app.handle()) {
                eprintln!("[tray] 创建托盘失败：{e}");
            }
            Ok(())
        })
        // 点窗口关闭按钮只最小化到托盘，真正退出走托盘菜单。
        .on_window_event(|window, event| tray::handle_window_event(window, event))
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_config,
            save_stage,
            delete_stage,
            reorder_stages,
            save_token,
            clear_token,
            token_status,
            save_service,
            delete_service,
            toggle_favorite,
            add_services,
            save_selection,
            save_template,
            preview_template,
            test_connection,
            list_remote_jobs,
            enqueue_tasks,
            cancel_task,
            queue_status,
            clear_finished_tasks,
            resend_notification,
            render_message,
            template_variables,
            validate_params,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

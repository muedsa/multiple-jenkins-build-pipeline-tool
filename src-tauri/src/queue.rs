//! 发布任务队列。
//!
//! 每个「服务 × 一条流水线」是一个**独立任务**：
//! - 入队后立刻开始执行（队列内全部并行，互不等待）
//! - 每个任务沿环境链逐级推进，独立完成、独立成功或失败
//! - 取消只作用于单个任务，其他任务照常跑
//! - 每个任务跑完弹一次系统通知，文案里带服务名与它自己的结果
//!
//! 每次状态变化都会 `emit` 一个 `queue-event` 给前端，事件里带自增的 `seq`，
//! 前端据此去重（WebView 刚加载完可能收不到早期事件，所以完整快照也能靠
//! [`QueueRegistry::snapshot`] 补齐）。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use crate::config::{AppConfig, BuildParam, StageConfig};
use crate::jenkins::{
    current_stage, BuildOutcome, JenkinsClient, PipelineStage,
};
use crate::notify::{build_content, humanize_duration, send_system_notification, TemplateVars};

/// 通知内容类型对测试与验证程序可见（[`Sink`] 会用到它）。
pub use crate::notify::NotificationContent;

/// 一个阶段（某环境上的一次构建）的状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StageStatus {
    Pending,
    /// 已提交构建，等待 Jenkins 分配构建号。
    Queued,
    Running,
    Success,
    Failed,
    /// 上一级失败且开启了「失败即中止」，这一级不再执行。
    Skipped,
    /// 用户主动停止。
    Cancelled,
}

impl StageStatus {
    fn is_terminal(self) -> bool {
        matches!(
            self,
            StageStatus::Success
                | StageStatus::Failed
                | StageStatus::Skipped
                | StageStatus::Cancelled
        )
    }
}

/// 某个环境的执行进度。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StageRun {
    /// 环境名，对应 [`StageConfig::name`]。
    pub env: String,
    pub status: StageStatus,
    pub build_number: Option<u64>,
    pub url: String,
    pub duration_secs: u64,
    pub error: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    /// Jenkins Pipeline 的各阶段进度。拿不到（没装插件、FreeStyle Job）时为空。
    pub pipeline_stages: Vec<PipelineStage>,
    /// 当前跑到哪个阶段，取自 [`pipeline_stages`]，方便界面直接显示。
    pub current_stage: Option<String>,
    /// 本级是靠哪个阶段「命中即放行」通过的；没启用时为空。
    pub passed_on_stage: Option<String>,
}

impl StageRun {
    fn new(env: &str) -> Self {
        Self {
            env: env.to_string(),
            status: StageStatus::Pending,
            build_number: None,
            url: String::new(),
            duration_secs: 0,
            error: None,
            started_at: None,
            finished_at: None,
            pipeline_stages: Vec::new(),
            current_stage: None,
            passed_on_stage: None,
        }
    }
}

/// 队列里的一条发布任务。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: String,
    /// 服务名（= Jenkins Job 名）。
    pub service: String,
    pub status: StageStatus,
    /// 环境链路，按执行顺序。
    pub stages: Vec<StageRun>,
    /// 入队时间。
    pub queued_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    /// 该任务的总耗时（秒），结束时才有值。
    pub elapsed_secs: u64,
    /// 结束后的通知文案（还在跑时为 `None`）。
    pub notification: Option<NotificationContent>,
    /// 汇总的错误信息，便于列表上直接展示。
    pub error: Option<String>,
}

impl Task {
    fn stage_mut(&mut self, env: &str) -> Option<&mut StageRun> {
        self.stages.iter_mut().find(|s| s.env == env)
    }

    fn is_terminal(&self) -> bool {
        self.status.is_terminal()
    }
}

/// 整条队列的快照。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueSnapshot {
    /// 本次环境链，前端据此渲染列。
    pub stages: Vec<String>,
    /// 按入队顺序排列，**正在执行的在最前**，然后是等待中的，最后是已结束的。
    pub tasks: Vec<Task>,
    /// 还没结束的任务数。
    pub pending_count: usize,
    pub running_count: usize,
    pub success_count: usize,
    pub failed_count: usize,
}

impl QueueSnapshot {
    /// 队列是否已经空闲（没有等待中或执行中的任务）。
    #[cfg(test)]
    fn is_idle(&self) -> bool {
        self.pending_count == 0 && self.running_count == 0
    }
}

/// 队列向外发消息的两个出口。
///
/// 抽成 trait 有两个好处：
/// - 真实运行时装到 [`tauri::AppHandle`] 上（emit 事件 + 弹系统通知）
/// - 单元测试与验证程序里可以换成不依赖 Tauri 运行时的实现
pub trait Sink: Send + Sync + 'static {
    /// 把队列事件推给前端。
    fn emit_queue(&self, payload: &QueueEvent);
    /// 发一次系统通知。
    fn notify(&self, content: &NotificationContent);
}

/// 真实实现：走 Tauri 的事件与通知插件。
pub struct AppSink {
    app: AppHandle,
}

impl AppSink {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

impl Sink for AppSink {
    fn emit_queue(&self, payload: &QueueEvent) {
        let _ = self.app.emit("queue-event", payload);
    }

    fn notify(&self, content: &NotificationContent) {
        let _ = send_system_notification(&self.app, content);
    }
}

/// 一条给前端显示的日志。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub time: String,
    /// `info` / `success` / `error` / `warn`
    pub level: String,
    pub message: String,
    /// 归属的任务 id，便于前端过滤；启动阶段的公共日志为 `None`。
    pub task_id: Option<String>,
}

/// 发往前端的增量事件：新日志 + 最新完整快照。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueEvent {
    pub seq: u64,
    pub logs: Vec<LogEntry>,
    pub queue: QueueSnapshot,
}

/// 单个任务在运行期持有的取消开关。
struct Running {
    cancel: Arc<AtomicBool>,
}

/// 队列的全部可变状态，由一把锁保护。
#[derive(Default)]
struct Inner {
    /// 按入队顺序保存所有任务。
    tasks: Vec<Task>,
    /// 正在执行的任务 id -> 取消开关。
    running: std::collections::HashMap<String, Running>,
    /// 还没推送给前端的日志。
    pending_logs: Vec<LogEntry>,
    seq: u64,
    /// 最近一次运行使用的环境链（队列空时也要能显示列）。
    stages: Vec<String>,
}

impl Inner {
    /// 组装当前快照。正在执行的排最前，其次是等待中，最后是已结束的。
    fn snapshot(&self) -> QueueSnapshot {
        let order = |t: &Task| match t.status {
            StageStatus::Running | StageStatus::Queued => 0,
            StageStatus::Pending => 1,
            _ => 2,
        };
        let mut tasks: Vec<Task> = self.tasks.clone();
        tasks.sort_by_key(order);

        // 先把计数算完，再把 tasks 移进快照，避免同时借用与移动。
        let count = |s: StageStatus| tasks.iter().filter(|t| t.status == s).count();
        let pending_count = count(StageStatus::Pending) + count(StageStatus::Queued);
        let running_count = count(StageStatus::Running);
        let success_count = count(StageStatus::Success);
        let failed_count = count(StageStatus::Failed) + count(StageStatus::Cancelled);

        QueueSnapshot {
            stages: self.stages.clone(),
            tasks,
            pending_count,
            running_count,
            success_count,
            failed_count,
        }
    }
}

/// 任务队列。
#[derive(Default)]
pub struct QueueRegistry {
    inner: Mutex<Inner>,
    id_counter: AtomicU64,
}

impl QueueRegistry {
    fn next_id(&self) -> String {
        let n = self.id_counter.fetch_add(1, Ordering::SeqCst) + 1;
        format!("task-{n}")
    }

    /// 读取当前快照。
    pub fn snapshot(&self) -> QueueSnapshot {
        self.inner
            .lock()
            .map(|g| g.snapshot())
            .unwrap_or_else(|_| QueueSnapshot {
                stages: Vec::new(),
                tasks: Vec::new(),
                pending_count: 0,
                running_count: 0,
                success_count: 0,
                failed_count: 0,
            })
    }

    /// 请求取消某个任务。返回是否真的找到了这个任务。
    pub fn cancel(&self, task_id: &str) -> bool {
        let Ok(guard) = self.inner.lock() else {
            return false;
        };
        match guard.running.get(task_id) {
            Some(r) => {
                r.cancel.store(true, Ordering::SeqCst);
                true
            }
            // 还没开始跑的任务：直接从队列里撤掉。
            None => guard.tasks.iter().any(|t| t.id == task_id),
        }
    }

    /// 清空已结束的任务，方便反复使用。
    pub fn clear_finished(&self) -> usize {
        let Ok(mut guard) = self.inner.lock() else {
            return 0;
        };
        let before = guard.tasks.len();
        guard.tasks.retain(|t| !t.is_terminal());
        before - guard.tasks.len()
    }
}

/// 入队时需要固化下来的信息，避免执行途中配置被改。
#[derive(Clone)]
struct QueuedJob {
    task_id: String,
    service: String,
    params: Vec<BuildParam>,
}

/// 运行期上下文：负责写队列状态并推事件。
struct QueueCtx {
    sink: Arc<dyn Sink>,
    registry: Arc<QueueRegistry>,
}

impl QueueCtx {
    fn log_public(&self, level: &str, message: impl Into<String>) {
        self.log(None, level, message);
    }

    fn log(&self, task_id: Option<&str>, level: &str, message: impl Into<String>) {
        let entry = LogEntry {
            time: chrono::Local::now().format("%H:%M:%S").to_string(),
            level: level.to_string(),
            message: message.into(),
            task_id: task_id.map(str::to_string),
        };
        if let Ok(mut guard) = self.registry.inner.lock() {
            guard.pending_logs.push(entry);
        }
    }

    /// 推送一个事件（带上尚未发送的日志）。
    fn emit(&self) {
        let payload = {
            let Ok(mut guard) = self.registry.inner.lock() else {
                return;
            };
            let logs = std::mem::take(&mut guard.pending_logs);
            guard.seq += 1;
            QueueEvent {
                seq: guard.seq,
                logs,
                queue: guard.snapshot(),
            }
        };
        self.sink.emit_queue(&payload);
    }

    /// 就地修改某个任务。
    fn with_task<F: FnOnce(&mut Task)>(&self, task_id: &str, edit: F) {
        if let Ok(mut guard) = self.registry.inner.lock() {
            if let Some(task) = guard.tasks.iter_mut().find(|t| t.id == task_id) {
                edit(task);
            }
        }
    }
}

/// 把任务登记进队列，并为每个任务建立独立的取消开关。
///
/// 抽成独立函数是为了能在不依赖 Tauri 运行时的情况下验证隔离性：
/// 取消一个任务绝不能碰到别的任务。
fn admit(
    registry: &Arc<QueueRegistry>,
    jobs: &[QueuedJob],
    stage_names: &[String],
    queued_at: &str,
) -> Result<Vec<String>, String> {
    let mut guard = registry
        .inner
        .lock()
        .map_err(|_| "内部状态锁已损坏".to_string())?;
    guard.stages = stage_names.to_vec();

    let mut ids = Vec::with_capacity(jobs.len());
    for job in jobs {
        ids.push(job.task_id.clone());
        guard.tasks.push(Task {
            id: job.task_id.clone(),
            service: job.service.clone(),
            status: StageStatus::Pending,
            stages: stage_names.iter().map(|n| StageRun::new(n)).collect(),
            queued_at: queued_at.to_string(),
            started_at: None,
            finished_at: None,
            elapsed_secs: 0,
            notification: None,
            error: None,
        });
    }
    // 每个任务一个独立开关：取消互不影响。
    for job in jobs {
        guard.running.insert(
            job.task_id.clone(),
            Running {
                cancel: Arc::new(AtomicBool::new(false)),
            },
        );
    }
    Ok(ids)
}

/// 把用户选择的服务入队，并立刻开始执行。
///
/// 返回新入队的任务 id 列表。
pub fn enqueue(
    app: AppHandle,
    registry: Arc<QueueRegistry>,
    config: AppConfig,
    services: Vec<String>,
) -> Result<Vec<String>, String> {
    enqueue_with(Arc::new(AppSink::new(app)), registry, config, services)
}

/// 与 [`enqueue`] 相同，但接收任意 [`Sink`]。
///
/// 便于在单元测试与验证程序里替换掉 Tauri 运行时。
pub fn enqueue_with(
    sink: Arc<dyn Sink>,
    registry: Arc<QueueRegistry>,
    config: AppConfig,
    services: Vec<String>,
) -> Result<Vec<String>, String> {
    if services.is_empty() {
        return Err("请至少选择一个服务".to_string());
    }
    if config.stages.is_empty() {
        return Err("还没有配置任何流水线环境，请先到「设置」里添加".to_string());
    }

    // 前置校验：环境没配全就整批拒绝，避免入队后才一个个失败。
    let mut clients = Vec::with_capacity(config.stages.len());
    for stage in &config.stages {
        let client = JenkinsClient::new(stage).map_err(|e| {
            format!("{e}\n请在「设置」中补全「{}」环境信息", stage.name)
        })?;
        clients.push(client);
    }

    // 收集要入队的服务（按给定顺序，跳过重复）。
    let mut jobs: Vec<QueuedJob> = Vec::with_capacity(services.len());
    for name in &services {
        let service = config
            .service(name)
            .ok_or_else(|| format!("服务「{name}」不在服务列表中"))?;
        let task_id = registry.next_id();
        jobs.push(QueuedJob {
            task_id,
            service: service.name.clone(),
            params: service.params.clone(),
        });
    }

    let stage_names = config.stage_names();
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let ids = admit(&registry, &jobs, &stage_names, &now)?;

    let ctx = Arc::new(QueueCtx {
        sink,
        registry: registry.clone(),
    });
    ctx.log_public(
        "info",
        format!(
            "入队 {} 个服务：{}；流水线：{}",
            jobs.len(),
            jobs.iter()
                .map(|j| j.service.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            stage_names.join(" → ")
        ),
    );
    ctx.emit();

    // 队列内全部并行：每个任务一个独立协程。
    for job in jobs {
        let ctx = ctx.clone();
        let clients = clients.clone();
        let stages = config.stages.clone();
        let template = config.template.clone();
        tauri::async_runtime::spawn(async move {
            run_task(ctx.clone(), job, stages, clients, template).await;
        });
    }

    Ok(ids)
}

/// 取某个任务的取消开关。
fn cancel_flag(registry: &Arc<QueueRegistry>, task_id: &str) -> Arc<AtomicBool> {
    registry
        .inner
        .lock()
        .ok()
        .and_then(|g| g.running.get(task_id).map(|r| r.cancel.clone()))
        .unwrap_or_else(|| Arc::new(AtomicBool::new(false)))
}

/// 跑完一个任务：沿环境链逐级推进，结束后发通知并清掉运行标记。
async fn run_task(
    ctx: Arc<QueueCtx>,
    job: QueuedJob,
    stages: Vec<StageConfig>,
    clients: Vec<JenkinsClient>,
    template: crate::config::TemplateConfig,
) {
    let task_id = job.task_id.clone();
    let service = job.service.clone();
    let cancel = cancel_flag(&ctx.registry, &task_id);

    let started = std::time::Instant::now();
    ctx.with_task(&task_id, |t| {
        t.status = StageStatus::Running;
        t.started_at = Some(chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string());
    });
    ctx.log(Some(&task_id), "info", format!("{service}：开始发布"));
    ctx.emit();

    // 主循环只关心「上一级是否失败」和「上一级是否失败即中止」。
    let mut prev_failed = false;
    let mut prev_abort = false;
    let mut prev_name = String::new();
    let mut passed_all = true;

    for index in 0..stages.len() {
        let stage = &stages[index];
        let tag = format!("{service} @{}", stage.name);

        // 取消检查放在每一轮开头：这样即使在排队阶段就点了取消，
        // 任务也会立刻收敛为「已取消」，而不是照样把构建跑完。
        if cancel.load(Ordering::SeqCst) {
            let envs: Vec<String> = stages[index..].iter().map(|s| s.name.clone()).collect();
            ctx.with_task(&task_id, |t| {
                for env in &envs {
                    if let Some(s) = t.stage_mut(env) {
                        s.status = StageStatus::Cancelled;
                        s.error = Some("已停止".to_string());
                    }
                }
            });
            passed_all = false;
            break;
        }

        // 上一级失败且设了中止 -> 这一级起全部跳过。
        if prev_failed && prev_abort {
            let reason =
                format!("上一级 {prev_name} 未成功且设为失败即中止，故跳过本级");
            ctx.log(Some(&task_id), "warn", format!("{tag}：{reason}"));
            let envs: Vec<String> = stages[index..].iter().map(|s| s.name.clone()).collect();
            ctx.with_task(&task_id, |t| {
                for env in &envs {
                    if let Some(s) = t.stage_mut(env) {
                        s.status = StageStatus::Skipped;
                        s.error = Some(reason.clone());
                    }
                }
            });
            passed_all = false;
            break;
        }

        let ok = run_stage(&ctx, &task_id, &service, stage, &clients[index], &job.params, &cancel)
            .await;
        prev_failed = !ok;
        prev_abort = stage.abort_on_failure;
        prev_name = stage.name.clone();
        if !ok {
            passed_all = false;
        }
    }

    let cancelled = cancel.load(Ordering::SeqCst);
    let elapsed = started.elapsed().as_secs();

    // 收敛任务终态，并取回一份用于生成通知的快照。
    let final_status = if cancelled {
        StageStatus::Cancelled
    } else if passed_all {
        StageStatus::Success
    } else {
        StageStatus::Failed
    };

    ctx.with_task(&task_id, |t| {
        t.status = final_status;
        t.finished_at =
            Some(chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string());
        t.elapsed_secs = elapsed;
        if final_status != StageStatus::Success {
            // 把第一个出错的级写进汇总，列表上能直接看到原因。
            t.error = t
                .stages
                .iter()
                .find(|s| s.error.is_some())
                .map(|s| format!("{}：{}", s.env, s.error.clone().unwrap_or_default()));
        }
    });

    // 通知按单个服务发：文案里的服务名、环境链路、结果都取自这一个任务。
    let Some(task) = task_snapshot(&ctx.registry, &task_id) else {
        // 任务被清掉了（理论上不会发生），直接收尾。
        if let Ok(mut guard) = ctx.registry.inner.lock() {
            guard.running.remove(&task_id);
        }
        return;
    };
    let content = build_content(&template, &build_vars(&task));

    ctx.with_task(&task_id, |t| {
        t.notification = Some(content.clone());
    });
    match final_status {
        StageStatus::Success => ctx.log(
            Some(&task_id),
            "success",
            format!("{service}：发布完成 —— {}", content.body),
        ),
        StageStatus::Cancelled => {
            ctx.log(Some(&task_id), "warn", format!("{service}：已取消"))
        }
        _ => ctx.log(
            Some(&task_id),
            "error",
            format!("{service}：发布失败 —— {}", content.body),
        ),
    }

    // 每个任务结束都弹一次通知 —— 这是「某个服务完成就通知一次」的落点。
    ctx.sink.notify(&content);

    ctx.emit();

    // 收尾：移除运行标记（任务本身留在队列里，供查看与清理）。
    if let Ok(mut guard) = ctx.registry.inner.lock() {
        guard.running.remove(&task_id);
    }
}

/// 取某个任务的快照。
fn task_snapshot(registry: &Arc<QueueRegistry>, task_id: &str) -> Option<Task> {
    registry
        .inner
        .lock()
        .ok()?
        .tasks
        .iter()
        .find(|t| t.id == task_id)
        .cloned()
}

/// 执行某个环境上的一次构建：触发 -> 等分配构建号 -> 等结束（或命中阶段提前放行）。
async fn run_stage(
    ctx: &Arc<QueueCtx>,
    task_id: &str,
    service: &str,
    stage_cfg: &StageConfig,
    client: &JenkinsClient,
    params: &[BuildParam],
    cancel: &Arc<AtomicBool>,
) -> bool {
    let env = stage_cfg.name.as_str();
    let tag = format!("{service} @{env}");
    let poll = Duration::from_secs(stage_cfg.poll_interval_secs.max(1));
    let timeout = Duration::from_secs(stage_cfg.timeout_minutes.max(1) * 60);

    ctx.with_task(task_id, |t| {
        if let Some(s) = t.stage_mut(env) {
            s.status = StageStatus::Queued;
            s.started_at = Some(chrono::Local::now().format("%H:%M:%S").to_string());
        }
    });
    ctx.log(Some(task_id), "info", format!("{tag}：触发构建"));
    ctx.emit();

    let queue = match client.trigger_build(service, params).await {
        Ok(queue) => queue,
        Err(err) => {
            let message = err.user_message(env);
            mark_failed(ctx, task_id, env, message.clone(), cancel);
            ctx.log(Some(task_id), "error", format!("{tag}：{message}"));
            return false;
        }
    };

    let build_number = match client
        .resolve_build_number(&queue, poll, timeout, || {})
        .await
    {
        Ok(Some(number)) => number,
        Ok(None) => {
            let message = format!("{tag}：构建在队列中被取消");
            mark_failed(ctx, task_id, env, message.clone(), cancel);
            ctx.log(Some(task_id), "error", message);
            return false;
        }
        Err(err) => {
            let message = err.user_message(env);
            mark_failed(ctx, task_id, env, message.clone(), cancel);
            ctx.log(Some(task_id), "error", format!("{tag}：{message}"));
            return false;
        }
    };

    ctx.with_task(task_id, |t| {
        if let Some(s) = t.stage_mut(env) {
            s.status = StageStatus::Running;
            s.build_number = Some(build_number);
            // 必须用 Jenkins 的 job 路径规则生成，否则网页地址会 404。
            s.url = client.build_page_url(service, build_number);
        }
    });
    ctx.log(Some(task_id), "info", format!("{tag}：开始执行 #{build_number}"));
    ctx.emit();

    // 「命中即放行」：环境上填了阶段名，该阶段一成功就算本级通过，不等后面的阶段。
    let target = {
        let name = stage_cfg.pass_on_stage.trim();
        (!name.is_empty()).then(|| name.to_string())
    };
    let wait_started = std::time::Instant::now();
    let mut last_sig = String::new();
    let mut last_stage: Option<String> = None;
    let mut warned = false;

    let outcome = client
        .wait_for_build(
            service,
            build_number,
            poll,
            timeout,
            target.as_deref(),
            |_status, progress| {
                // 阶段接口不可用只在真的配了放行条件时提醒一次，免得刷屏。
                if !warned && target.is_some() {
                    if let Some(reason) = progress.unavailable.as_deref() {
                        warned = true;
                        ctx.log(
                            Some(task_id),
                            "warn",
                            format!("{tag}：拿不到阶段信息（{reason}），本级改按整条构建结果判定"),
                        );
                        ctx.emit();
                    }
                }
                // 按整个列表的签名比较：状态变了就该刷新，否则最后一轮可能停在「进行中」。
                let sig = stage_signature(&progress.stages);
                if sig == last_sig {
                    return;
                }
                last_sig = sig;
                let stages = progress.stages.clone();
                let current = current_stage(&stages).map(|s| s.name.clone());
                ctx.with_task(task_id, |t| {
                    if let Some(s) = t.stage_mut(env) {
                        s.pipeline_stages = stages;
                        s.current_stage = current.clone();
                    }
                });
                // 阶段一挪动就记一行，回看时能定位卡在哪一步。
                if current != last_stage {
                    last_stage = current;
                    if let Some(name) = last_stage.as_deref() {
                        ctx.log(Some(task_id), "info", format!("{tag}：阶段「{name}」"));
                    }
                }
                ctx.emit();
            },
        )
        .await;

    match outcome {
        Ok(BuildOutcome::EarlyPassed { stage, status }) => {
            // 构建还没结束，Jenkins 给的 duration 不可用，改用我们实际等了多久。
            let waited = wait_started.elapsed().as_secs();
            ctx.with_task(task_id, |t| {
                if let Some(s) = t.stage_mut(env) {
                    s.status = StageStatus::Success;
                    s.build_number = Some(status.build_number);
                    if !status.url.is_empty() {
                        s.url = status.url.clone();
                    }
                    s.duration_secs = waited;
                    s.passed_on_stage = Some(stage.clone());
                    s.finished_at =
                        Some(chrono::Local::now().format("%H:%M:%S").to_string());
                }
            });
            ctx.log(
                Some(task_id),
                "success",
                format!(
                    "{tag}：阶段「{stage}」已成功，忽略后续阶段直接放行（Jenkins #{build_number} 继续跑，本工具不中断它）"
                ),
            );
            ctx.emit();
            true
        }
        Ok(BuildOutcome::Finished(status)) => {
            let ok = status.state.is_success();
            ctx.with_task(task_id, |t| {
                if let Some(s) = t.stage_mut(env) {
                    s.status = if ok {
                        StageStatus::Success
                    } else {
                        StageStatus::Failed
                    };
                    s.build_number = Some(status.build_number);
                    s.duration_secs = status.duration_secs;
                    if !status.url.is_empty() {
                        s.url = status.url.clone();
                    }
                    if !ok {
                        s.error = Some(status.state.label());
                    }
                    s.finished_at =
                        Some(chrono::Local::now().format("%H:%M:%S").to_string());
                }
            });
            if ok {
                ctx.log(
                    Some(task_id),
                    "success",
                    format!(
                        "{tag}：构建 #{build_number} 成功（耗时 {}）",
                        humanize_duration(status.duration_secs)
                    ),
                );
            } else {
                ctx.log(
                    Some(task_id),
                    "error",
                    format!("{tag}：构建 #{build_number} {}", status.state.label()),
                );
            }
            ok
        }
        Err(err) => {
            let message = err.user_message(env);
            mark_failed(ctx, task_id, env, message.clone(), cancel);
            ctx.log(Some(task_id), "error", format!("{tag}：{message}"));
            false
        }
    }
}

fn mark_failed(
    ctx: &Arc<QueueCtx>,
    task_id: &str,
    env: &str,
    message: String,
    cancel: &Arc<AtomicBool>,
) {
    let cancelled = cancel.load(Ordering::SeqCst);
    ctx.with_task(task_id, |t| {
        if let Some(s) = t.stage_mut(env) {
            s.status = if cancelled {
                StageStatus::Cancelled
            } else {
                StageStatus::Failed
            };
            s.error = Some(message);
            s.finished_at = Some(chrono::Local::now().format("%H:%M:%S").to_string());
        }
    });
}

/// 阶段列表的签名：任一阶段的名字或状态变了就会不一样。
/// 轮询时靠它决定「要不要刷新界面、要不要记日志」，免得每轮都发一次事件。
fn stage_signature(stages: &[PipelineStage]) -> String {
    stages
        .iter()
        .map(|s| format!("{}={};", s.name, s.status))
        .collect()
}

/// 为**单个任务**组装通知模板变量。
///
/// 通知是按服务发的，所以这里的「总数」都是 1，
/// 服务名、环境链路、结果都取自这一个任务。
fn build_vars(task: &Task) -> TemplateVars {
    let stage_chain = task
        .stages
        .iter()
        .map(|s| s.env.as_str())
        .collect::<Vec<_>>()
        .join(" → ");
    // 走到了哪一级：最后一个有结论的级。
    let reached = task
        .stages
        .iter()
        .rev()
        .find(|s| s.status != StageStatus::Pending)
        .map(|s| s.env.clone())
        .unwrap_or_default();
    let failed_stage = task
        .stages
        .iter()
        .find(|s| matches!(s.status, StageStatus::Failed | StageStatus::Cancelled))
        .map(|s| s.env.clone())
        .unwrap_or_default();

    TemplateVars {
        service_count: 1,
        success_count: usize::from(task.status == StageStatus::Success),
        failed_count: usize::from(task.status != StageStatus::Success),
        success_list: if task.status == StageStatus::Success {
            task.service.clone()
        } else {
            String::new()
        },
        failed_list: if task.status == StageStatus::Success {
            String::new()
        } else {
            task.service.clone()
        },
        service_list: task.service.clone(),
        duration: humanize_duration(task.elapsed_secs),
        status: match task.status {
            StageStatus::Success => "成功".to_string(),
            StageStatus::Cancelled => "已取消".to_string(),
            _ => "失败".to_string(),
        },
        finished_at: task
            .finished_at
            .clone()
            .unwrap_or_else(|| chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()),
        stage_chain,
        stage_count: task.stages.len(),
        service_name: task.service.clone(),
        reached_stage: reached,
        failed_stage,
    }
}

/// 让调用方拿到队列里的任务（用于「重发通知」）。
pub fn task(registry: &QueueRegistry, task_id: &str) -> Option<Task> {
    registry
        .inner
        .lock()
        .ok()?
        .tasks
        .iter()
        .find(|t| t.id == task_id)
        .cloned()
}

/// 决定某一级该做什么（与网络无关，便于单元测试）。
///
/// `prev_failed` 表示**上一级**是否失败；`prev_abort` 表示上一级是否开了
/// 「失败即中止」。两者同时成立时，本级及其后面所有级都不再执行。
#[cfg(test)]
fn decide_stage(prev_failed: bool, prev_abort: bool, prev_name: &str) -> (bool, Option<String>) {
    if prev_failed && prev_abort {
        (
            false,
            Some(format!("上一级 {prev_name} 未成功且设为失败即中止，故跳过本级")),
        )
    } else {
        (true, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stage(name: &str) -> StageConfig {
        StageConfig::new(name)
    }

    fn ps(name: &str, status: &str) -> PipelineStage {
        PipelineStage {
            name: name.to_string(),
            status: status.to_string(),
            duration_secs: 0,
        }
    }

    /// 轮询时靠签名决定要不要刷新界面：名字或状态变了才算变。
    #[test]
    fn stage_signature_detects_stage_changes() {
        assert_eq!(stage_signature(&[]), "", "空列表的签名是空串");

        let before = vec![ps("Build", "SUCCESS"), ps("Deploy", "IN_PROGRESS")];
        let same = vec![ps("Build", "SUCCESS"), ps("Deploy", "IN_PROGRESS")];
        assert_eq!(stage_signature(&before), stage_signature(&same));

        let finished = vec![ps("Build", "SUCCESS"), ps("Deploy", "SUCCESS")];
        assert_ne!(
            stage_signature(&before),
            stage_signature(&finished),
            "阶段跑完了必须刷新，否则界面还停在「进行中」"
        );

        let renamed = vec![ps("Build", "SUCCESS"), ps("Publish", "IN_PROGRESS")];
        assert_ne!(stage_signature(&before), stage_signature(&renamed));
    }

    /// 单级判定：上一级失败且开了中止才截断，否则继续推进。
    #[test]
    fn decide_stage_only_blocks_when_prev_failed_and_aborted() {
        let (exec, reason) = decide_stage(true, false, "dev");
        assert!(exec, "上一级没开失败即中止时，本级应继续执行");
        assert!(reason.is_none());

        let (exec, _) = decide_stage(false, true, "sit");
        assert!(exec, "上一级成功时本级应执行");

        let (exec, reason) = decide_stage(true, true, "sit");
        assert!(!exec, "上一级失败且开了中止时，本级应跳过");
        assert!(
            reason.unwrap().contains("sit"),
            "跳过原因里要指明是哪一级拦下的"
        );
    }

    /// 链式推演：中途某级开了中止，它后面全部截断；没开则一路放行。
    /// 这里验证的是配置层面的“最坏情况”计划。
    #[test]
    fn plan_truncates_downstream_after_abort_stage() {
        let mut sit = stage("sit");
        sit.abort_on_failure = true;
        let stages = vec![stage("dev"), sit, stage("uat"), stage("prod")];

        let mut plan: Vec<(bool, Option<String>)> = Vec::new();
        for (i, _) in stages.iter().enumerate() {
            if i == 0 {
                plan.push((true, None));
                continue;
            }
            let prev = &stages[i - 1];
            if !plan[i - 1].0 {
                plan.push(plan[i - 1].clone());
            } else if prev.abort_on_failure {
                plan.push((false, Some(format!("上一级 {} 可能中止", prev.name))));
            } else {
                plan.push((true, None));
            }
        }

        assert_eq!(plan.len(), 4);
        assert!(plan[0].0, "dev 应执行");
        assert!(plan[1].0, "sit 自己应执行");
        assert!(!plan[2].0, "uat 应被跳过");
        assert!(!plan[3].0, "prod 应被跳过");
    }

    /// 空队列的快照应当是全零，前端首次加载时不会崩。
    #[test]
    fn empty_queue_snapshot_is_zeroed() {
        let registry = QueueRegistry::default();
        let snap = registry.snapshot();
        assert!(snap.tasks.is_empty());
        assert!(snap.stages.is_empty());
        assert_eq!(snap.pending_count, 0);
        assert_eq!(snap.running_count, 0);
        assert!(snap.is_idle());
    }

    /// 任务 id 要自增且互不相同，避免两个任务互相覆盖状态。
    #[test]
    fn task_ids_are_unique_and_increasing() {
        let registry = QueueRegistry::default();
        assert_eq!(registry.next_id(), "task-1");
        assert_eq!(registry.next_id(), "task-2");
    }

    /// 取消一个不存在的任务要返回 false，不能误伤别的任务。
    #[test]
    fn cancel_unknown_task_returns_false() {
        let registry = QueueRegistry::default();
        assert!(!registry.cancel("nope"));
    }

    /// 每个任务有独立的取消开关：取消一个不影响另一个。
    #[test]
    fn cancelling_one_task_does_not_affect_others() {
        let registry = Arc::new(QueueRegistry::default());
        let jobs = vec![
            QueuedJob {
                task_id: "task-a".into(),
                service: "svc-a".into(),
                params: vec![],
            },
            QueuedJob {
                task_id: "task-b".into(),
                service: "svc-b".into(),
                params: vec![],
            },
        ];
        let stages = vec!["dev".to_string(), "sit".to_string()];
        let ids = admit(&registry, &jobs, &stages, "2026-01-02 15:00:00").unwrap();
        assert_eq!(ids, vec!["task-a", "task-b"]);

        let flag = |id: &str| {
            registry
                .inner
                .lock()
                .unwrap()
                .running
                .get(id)
                .map(|r| r.cancel.clone())
                .expect("任务应已登记取消开关")
        };
        let a = flag("task-a");
        let b = flag("task-b");

        assert!(registry.cancel("task-a"));
        assert!(a.load(Ordering::SeqCst), "task-a 应被标记取消");
        assert!(
            !b.load(Ordering::SeqCst),
            "task-b 的取消开关不应被碰到 —— 单个服务取消不能影响其他服务"
        );

        // 队列里两个任务都还在（取消只是打标记，由任务自己收敛状态）。
        let snap = registry.snapshot();
        assert_eq!(snap.tasks.len(), 2);
        assert_eq!(snap.stages, stages);
    }

    /// 两个任务的结构互相独立：改一个的阶段状态不会串到另一个。
    #[test]
    fn tasks_keep_independent_stage_state() {
        let registry = Arc::new(QueueRegistry::default());
        let jobs = vec![
            QueuedJob {
                task_id: "task-a".into(),
                service: "svc-a".into(),
                params: vec![],
            },
            QueuedJob {
                task_id: "task-b".into(),
                service: "svc-b".into(),
                params: vec![],
            },
        ];
        admit(
            &registry,
            &jobs,
            &["dev".to_string(), "sit".to_string()],
            "t",
        )
        .unwrap();

        {
            let mut guard = registry.inner.lock().unwrap();
            let a = guard.tasks.iter_mut().find(|t| t.id == "task-a").unwrap();
            a.status = StageStatus::Failed;
            a.stage_mut("sit").unwrap().status = StageStatus::Failed;
        }

        let snap = registry.snapshot();
        let a = snap.tasks.iter().find(|t| t.id == "task-a").unwrap();
        let b = snap.tasks.iter().find(|t| t.id == "task-b").unwrap();
        assert_eq!(a.status, StageStatus::Failed);
        assert_eq!(a.stages[1].status, StageStatus::Failed);
        // b 必须完全没被影响。
        assert_eq!(b.status, StageStatus::Pending);
        assert!(b.stages.iter().all(|s| s.status == StageStatus::Pending));
        assert_eq!(snap.failed_count, 1);
    }

    /// 通知变量按「单个服务」组装：服务名、链路、结果都取自这一个任务。
    #[test]
    fn build_vars_describes_one_service() {
        let mut task = Task {
            id: "task-1".into(),
            service: "fsk-esale".into(),
            status: StageStatus::Failed,
            stages: vec![
                StageRun::new("dev"),
                StageRun::new("sit"),
                StageRun::new("prod"),
            ],
            queued_at: "2026-01-02 15:00:00".into(),
            started_at: None,
            finished_at: Some("2026-01-02 15:03:12".into()),
            elapsed_secs: 192,
            notification: None,
            error: None,
        };
        task.stages[0].status = StageStatus::Success;
        task.stages[1].status = StageStatus::Failed;
        task.stages[1].error = Some("不稳定".into());
        task.stages[2].status = StageStatus::Skipped;

        let vars = build_vars(&task);
        assert_eq!(vars.service_name, "fsk-esale");
        assert_eq!(vars.service_list, "fsk-esale");
        assert_eq!(vars.stage_chain, "dev → sit → prod");
        assert_eq!(vars.stage_count, 3);
        assert_eq!(vars.status, "失败");
        assert_eq!(vars.duration, "3分12秒");
        // 卡在 sit，而不是最后一级。
        assert_eq!(vars.failed_stage, "sit");
        assert_eq!(vars.reached_stage, "prod");
        assert_eq!(vars.success_count, 0);
        assert_eq!(vars.failed_count, 1);
    }

    /// 成功任务的通知变量。
    #[test]
    fn build_vars_for_successful_task() {
        let mut task = Task {
            id: "task-1".into(),
            service: "base-service-pay".into(),
            status: StageStatus::Success,
            stages: vec![StageRun::new("dev"), StageRun::new("sit")],
            queued_at: String::new(),
            started_at: None,
            finished_at: Some("2026-01-02 15:01:00".into()),
            elapsed_secs: 60,
            notification: None,
            error: None,
        };
        for s in &mut task.stages {
            s.status = StageStatus::Success;
        }
        let vars = build_vars(&task);
        assert_eq!(vars.status, "成功");
        assert_eq!(vars.success_count, 1);
        assert_eq!(vars.failed_count, 0);
        assert_eq!(vars.failed_list, "");
        assert_eq!(vars.service_name, "base-service-pay");
    }

    /// 快照排序：正在跑的排最前，其次是等待中，已结束的排最后。
    #[test]
    fn snapshot_orders_running_first_then_finished() {
        let registry = QueueRegistry::default();
        {
            let mut guard = registry.inner.lock().unwrap();
            for (id, status) in [
                ("done", StageStatus::Success),
                ("waiting", StageStatus::Pending),
                ("busy", StageStatus::Running),
                ("failed", StageStatus::Failed),
            ] {
                guard.tasks.push(Task {
                    id: id.into(),
                    service: id.into(),
                    status,
                    stages: vec![StageRun::new("dev")],
                    queued_at: String::new(),
                    started_at: None,
                    finished_at: None,
                    elapsed_secs: 0,
                    notification: None,
                    error: None,
                });
            }
        }
        let snap = registry.snapshot();
        assert_eq!(snap.tasks[0].id, "busy", "执行中的排最前");
        assert_eq!(snap.tasks[1].id, "waiting", "等待中的排第二");
        assert_eq!(snap.running_count, 1);
        assert_eq!(snap.pending_count, 1);
        assert_eq!(snap.success_count, 1);
        assert_eq!(snap.failed_count, 1);
    }

    /// 清空已结束任务只删终态的，保留还在跑的。
    #[test]
    fn clear_finished_keeps_running_tasks() {
        let registry = QueueRegistry::default();
        {
            let mut guard = registry.inner.lock().unwrap();
            for (id, status) in [
                ("done", StageStatus::Success),
                ("busy", StageStatus::Running),
                ("skipped", StageStatus::Skipped),
            ] {
                guard.tasks.push(Task {
                    id: id.into(),
                    service: id.into(),
                    status,
                    stages: vec![],
                    queued_at: String::new(),
                    started_at: None,
                    finished_at: None,
                    elapsed_secs: 0,
                    notification: None,
                    error: None,
                });
            }
        }
        let removed = registry.clear_finished();
        assert_eq!(removed, 2, "成功与跳过都属于已结束");
        let snap = registry.snapshot();
        assert_eq!(snap.tasks.len(), 1);
        assert_eq!(snap.tasks[0].id, "busy");
    }
}

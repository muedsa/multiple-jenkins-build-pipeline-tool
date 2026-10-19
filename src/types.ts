/** 与 Rust 后端一一对应的数据类型。字段名保持 camelCase，和后端 serde 输出一致。 */

/** 环境名。任意字符串，例如 dev / sit / prod。 */
export type EnvName = string;

/** 每个阶段/服务的状态。 */
export type StageStatus =
  | "pending"
  | "queued"
  | "running"
  | "success"
  | "failed"
  | "skipped"
  | "cancelled";

export interface BuildParam {
  key: string;
  value: string;
}

/** 一个可发布的服务。约定各环境上是同名 Job。 */
export interface Service {
  name: string;
  favorite: boolean;
  params: BuildParam[];
}

/**
 * 一个流水线环境（一级）。数组顺序即执行顺序。
 *
 * `name` 既是显示名也是环境标识，同时是凭据管理器里的键 ——
 * 改名等于换一把凭据，后端会自动把 token 迁过去。
 */
export interface StageConfig {
  name: string;
  baseUrl: string;
  username: string;
  /** 内网自签名证书时需要打开，会关闭 TLS 校验。 */
  insecureTls: boolean;
  pollIntervalSecs: number;
  timeoutMinutes: number;
  /** 本级失败后是否中止整条流水线（后面所有级都不再执行）。 */
  abortOnFailure: boolean;
  /**
   * 「命中即放行」的 Jenkins Pipeline 阶段名。
   * 该阶段一成功就算本级成功，直接进下一级，不等后面的阶段跑完。
   * 留空表示按整条构建的结果判定。
   */
  passOnStage: string;
}

/** Jenkins Pipeline 的一个阶段（来自 Pipeline Stage View 插件的 wfapi）。 */
export interface PipelineStage {
  name: string;
  /** 原样保留 Jenkins 的状态串。 */
  status: string;
  /** 该阶段耗时（秒），进行中或未跑到时为 0。 */
  durationSecs: number;
}

/** 某个环境是否已经存好 token。 */
export interface StageToken {
  name: string;
  hasToken: boolean;
}

export interface TemplateConfig {
  title: string;
  body: string;
  useFailureTemplate: boolean;
  failureTitle: string;
  failureBody: string;
}

export interface AppConfig {
  version: number;
  /** 流水线环境，按数组顺序依次执行。 */
  stages: StageConfig[];
  services: Service[];
  template: TemplateConfig;
  lastSelection: string[];
}

/** 某个服务在某一级环境上的进度。 */
export interface StageRun {
  /** 环境名。 */
  env: string;
  status: StageStatus;
  buildNumber: number | null;
  url: string;
  durationSecs: number;
  error: string | null;
  startedAt: string | null;
  finishedAt: string | null;
  /** Jenkins Pipeline 的各阶段进度；拿不到时为空数组。 */
  pipelineStages: PipelineStage[];
  /** 当前跑到的阶段名。 */
  currentStage: string | null;
  /** 本级靠哪个阶段「命中即放行」通过的；未启用时为 null。 */
  passedOnStage: string | null;
}

/** 队列里的一条发布任务：一个服务 × 一条流水线。 */
export interface Task {
  id: string;
  /** 服务名（= Jenkins Job 名）。 */
  service: string;
  status: StageStatus;
  /** 与环境链同序，逐级对应。 */
  stages: StageRun[];
  queuedAt: string;
  startedAt: string | null;
  finishedAt: string | null;
  elapsedSecs: number;
  /** 结束后的通知文案（还在跑时为 null）。 */
  notification: NotificationContent | null;
  /** 汇总的错误信息，列表上直接展示。 */
  error: string | null;
}

/** 整条队列的快照。 */
export interface QueueSnapshot {
  /** 环境链，用于渲染列。 */
  stages: string[];
  /** 执行中的排最前，其次是等待中，最后是已结束的。 */
  tasks: Task[];
  pendingCount: number;
  runningCount: number;
  successCount: number;
  failedCount: number;
}

export interface NotificationContent {
  title: string;
  body: string;
  failed: boolean;
}

export interface LogEntry {
  time: string;
  level: "info" | "success" | "error" | "warn" | string;
  message: string;
  /** 归属的任务 id；启动阶段的公共日志为 null。 */
  taskId: string | null;
}

/** 后端推来的增量事件：新日志 + 最新完整队列快照。 */
export interface QueueEvent {
  seq: number;
  logs: LogEntry[];
  queue: QueueSnapshot;
}

export interface ProbeResult {
  ok: boolean;
  message: string;
  version: string | null;
  mode: string | null;
}

/** 从 Jenkins 拉到的可构建 Job，用于搜索添加服务。 */
export interface RemoteJob {
  /** 带文件夹层级的完整名，也是触发时该用的 Job 名。 */
  name: string;
  /** 展示用的末端名字。 */
  displayName: string;
  url: string;
}

/** 各环境的 token 状态，与配置里的环境链同序。 */
export type TokenStatus = StageToken[];

export interface TemplateVariable {
  key: string;
  description: string;
}

export interface PreviewContent {
  successTitle: string;
  successBody: string;
  failureTitle: string;
  failureBody: string;
}

/** 状态对应的中文文案与配色 class。 */
export const STATUS_LABEL: Record<StageStatus, string> = {
  pending: "等待",
  queued: "排队中",
  running: "执行中",
  success: "成功",
  failed: "失败",
  skipped: "已跳过",
  cancelled: "已取消",
};

/** Jenkins 阶段状态的中文文案与符号。状态串来自 wfapi，未知值原样显示。 */
export const PIPELINE_STATUS: Record<string, { label: string; mark: string }> = {
  SUCCESS: { label: "成功", mark: "✓" },
  IN_PROGRESS: { label: "进行中", mark: "●" },
  FAILED: { label: "失败", mark: "✕" },
  ABORTED: { label: "已中断", mark: "⊘" },
  NOT_EXECUTED: { label: "未执行", mark: "○" },
  PAUSED_PENDING_INPUT: { label: "等待确认", mark: "⏸" },
};

/** 是否已经结束（不会再变化）。 */
export function isTerminal(status: StageStatus): boolean {
  return (
    status === "success" ||
    status === "failed" ||
    status === "skipped" ||
    status === "cancelled"
  );
}

/** 把秒数格式化成易读形式，和后端 humanize_duration 保持一致。 */
export function humanizeDuration(totalSecs: number): string {
  const hours = Math.floor(totalSecs / 3600);
  const minutes = Math.floor((totalSecs % 3600) / 60);
  const seconds = Math.floor(totalSecs % 60);
  if (hours > 0) return `${hours}小时${minutes}分${seconds}秒`;
  if (minutes > 0) return `${minutes}分${seconds}秒`;
  return `${seconds}秒`;
}

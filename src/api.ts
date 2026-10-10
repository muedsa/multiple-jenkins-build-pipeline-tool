/** 对 Tauri 命令的薄封装。所有命令都在 `src-tauri/src/lib.rs` 里注册。 */

import { invoke } from "@tauri-apps/api/core";
import type {
  AppConfig,
  PreviewContent,
  ProbeResult,
  QueueSnapshot,
  RemoteJob,
  Service,
  StageConfig,
  TemplateConfig,
  TemplateVariable,
  TokenStatus,
} from "./types";

/** 把 invoke 抛出的错误统一成可展示的字符串。 */
export function errorText(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return String(error);
}

export function getConfig(): Promise<AppConfig> {
  return invoke("get_config");
}

export function saveConfig(config: AppConfig): Promise<AppConfig> {
  return invoke("save_config", { config });
}

/** 新增或更新一个环境。`originalName` 非空表示改名（token 会一起迁移）。 */
export function saveStage(
  stage: StageConfig,
  originalName?: string,
): Promise<AppConfig> {
  return invoke("save_stage", {
    stage,
    originalName: originalName ?? null,
  });
}

export function deleteStage(name: string): Promise<AppConfig> {
  return invoke("delete_stage", { name });
}

/** 调整环境顺序（数组顺序即执行顺序）。 */
export function reorderStages(names: string[]): Promise<AppConfig> {
  return invoke("reorder_stages", { names });
}

export function saveToken(stage: string, token: string): Promise<void> {
  return invoke("save_token", { stage, token });
}

export function clearToken(stage: string): Promise<void> {
  return invoke("clear_token", { stage });
}

export function tokenStatus(): Promise<TokenStatus> {
  return invoke("token_status");
}

export function saveService(
  service: Service,
  originalName?: string,
): Promise<AppConfig> {
  return invoke("save_service", {
    service,
    originalName: originalName ?? null,
  });
}

export function deleteService(name: string): Promise<AppConfig> {
  return invoke("delete_service", { name });
}

export function toggleFavorite(name: string): Promise<AppConfig> {
  return invoke("toggle_favorite", { name });
}

export function addServices(names: string[]): Promise<AppConfig> {
  return invoke("add_services", { names });
}

export function saveSelection(names: string[]): Promise<void> {
  return invoke("save_selection", { names });
}

export function saveTemplate(template: TemplateConfig): Promise<AppConfig> {
  return invoke("save_template", { template });
}

export function previewTemplate(template: TemplateConfig): Promise<PreviewContent> {
  return invoke("preview_template", { template });
}

export function testConnection(stage: StageConfig): Promise<ProbeResult> {
  return invoke("test_connection", { stage });
}

/** 拉取某个环境上所有可构建的 Job。不传就用流水线里的第一个环境。 */
export function listRemoteJobs(stage?: string): Promise<RemoteJob[]> {
  return invoke("list_remote_jobs", { stage: stage ?? null });
}

/** 把选中的服务加入发布队列，队列内全部并行执行。返回新任务的 id。 */
export function enqueueTasks(services: string[]): Promise<string[]> {
  return invoke("enqueue_tasks", { services });
}

/** 取消单个任务。其他任务与队列不受影响。 */
export function cancelTask(taskId: string): Promise<boolean> {
  return invoke("cancel_task", { taskId });
}

/** 读取队列当前快照（页面刚打开时用它补齐状态）。 */
export function queueStatus(): Promise<QueueSnapshot> {
  return invoke("queue_status");
}

/** 清空已结束的任务。 */
export function clearFinishedTasks(): Promise<number> {
  return invoke("clear_finished_tasks");
}

export function resendNotification(taskId: string): Promise<void> {
  return invoke("resend_notification", { taskId });
}

export function templateVariables(): Promise<TemplateVariable[]> {
  return invoke("template_variables");
}

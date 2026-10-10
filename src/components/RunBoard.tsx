import type { StageRun, StageStatus, Task } from "../types";
import { STATUS_LABEL, humanizeDuration, isTerminal } from "../types";

/** 状态徽章。 */
export function StatusBadge({
  status,
  size = "normal",
}: {
  status: StageStatus;
  size?: "normal" | "small";
}) {
  return (
    <span className={`badge badge-${status} badge-${size}`}>
      {status === "running" && <span className="spinner" aria-hidden />}
      {STATUS_LABEL[status]}
    </span>
  );
}

/** 单条环境流水线。 */
function StageRow({ stage }: { stage: StageRun }) {
  const isLink = stage.url.startsWith("http");
  return (
    <div className={`stage stage-${stage.status}`}>
      <span className="stage-env">{stage.env}</span>
      <StatusBadge status={stage.status} size="small" />
      <span className="stage-meta">
        {stage.buildNumber != null && <span className="mono">#{stage.buildNumber}</span>}
        {stage.durationSecs > 0 && <span>{humanizeDuration(stage.durationSecs)}</span>}
        {stage.status === "running" && stage.startedAt && (
          <span className="muted">开始于 {stage.startedAt}</span>
        )}
      </span>
      {isLink && (
        <a
          className="stage-link"
          href={stage.url}
          target="_blank"
          rel="noreferrer"
          title="在浏览器中打开构建页面"
        >
          日志 ↗
        </a>
      )}
    </div>
  );
}

/**
 * 队列里的一条任务卡片。
 *
 * 每条任务都是独立的：可以单独取消，取消后其他任务照常跑。
 */
export function TaskCard({
  task,
  onCancel,
  onResend,
  busy,
}: {
  task: Task;
  onCancel: (id: string) => void;
  onResend: (id: string) => void;
  busy: boolean;
}) {
  const running =
    task.status === "pending" ||
    task.status === "queued" ||
    task.status === "running";

  return (
    <article className={`card card-${task.status}`}>
      <header className="card-head">
        <h3 title={task.service}>{task.service}</h3>
        <div className="card-head-right">
          <StatusBadge status={task.status} />
          {running ? (
            <button
              className="icon-btn danger"
              title="取消这个任务（不影响其他任务）"
              disabled={busy}
              onClick={() => onCancel(task.id)}
            >
              ⊘
            </button>
          ) : (
            <button
              className="icon-btn"
              title="重发这个任务的通知"
              disabled={busy || !task.notification}
              onClick={() => onResend(task.id)}
            >
              ↻
            </button>
          )}
        </div>
      </header>

      {task.stages.map((stage) => (
        <StageRow key={stage.env} stage={stage} />
      ))}

      <footer className="card-foot">
        <span className="muted small">
          {task.finishedAt
            ? `${task.finishedAt} 结束 · 耗时 ${humanizeDuration(task.elapsedSecs)}`
            : `入队于 ${task.queuedAt}`}
        </span>
      </footer>

      {/* 出错时把原因摊在卡片底部，便于一眼定位是哪一级挂了 */}
      {task.error && (
        <p className="card-error" title={task.error}>
          {task.error}
        </p>
      )}
    </article>
  );
}

/** 队列为空时的占位提示。 */
export function EmptyQueue({ hasSelection }: { hasSelection: boolean }) {
  return (
    <p className="empty">
      {hasSelection
        ? "点「加入队列」把选中的服务排进队列。"
        : "还没有任务。先在左侧勾选服务，再点「加入队列」。"}
    </p>
  );
}

/** 队列整体是否已经跑完（用于显示汇总）。 */
export function queueIsIdle(tasks: Task[]): boolean {
  return tasks.every((t) => isTerminal(t.status));
}

/** 状态文案，供外部复用。 */
export { STATUS_LABEL };

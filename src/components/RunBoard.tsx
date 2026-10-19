import type { StageRun, StageStatus, Task } from "../types";
import {
  PIPELINE_STATUS,
  STATUS_LABEL,
  humanizeDuration,
  isTerminal,
} from "../types";

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

/** 阶段状态对应的配色 class。 */
const PIPELINE_TONE: Record<string, string> = {
  SUCCESS: "ok",
  IN_PROGRESS: "run",
  FAILED: "bad",
  ABORTED: "warn",
  PAUSED_PENDING_INPUT: "warn",
};

/** Jenkins Pipeline 的阶段明细。拿不到（没装插件、FreeStyle Job）时不渲染。 */
function PipelineStrip({ stage }: { stage: StageRun }) {
  if (stage.pipelineStages.length === 0) return null;
  return (
    <div
      className="pipe-stages"
      title="Jenkins Pipeline 各阶段进度（来自 Pipeline Stage View 插件）"
    >
      {stage.pipelineStages.map((item, index) => {
        const meta = PIPELINE_STATUS[item.status] ?? { label: item.status, mark: "•" };
        const tone = PIPELINE_TONE[item.status] ?? "idle";
        const current = stage.currentStage === item.name;
        return (
          <span
            key={`${item.name}-${index}`}
            className={`pipe-stage ps-${tone}${current ? " is-current" : ""}`}
            title={`${item.name}：${meta.label}`}
          >
            <span className="pipe-stage-mark">{meta.mark}</span>
            {item.name}
            {item.durationSecs > 0 && (
              <span className="pipe-stage-time">{humanizeDuration(item.durationSecs)}</span>
            )}
          </span>
        );
      })}
    </div>
  );
}

/** 单条环境流水线。 */
function StageRow({ stage }: { stage: StageRun }) {
  const isLink = stage.url.startsWith("http");
  return (
    <div className="stage-block">
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

      <PipelineStrip stage={stage} />

      {stage.passedOnStage && (
        <span
          className="pipe-passed"
          title="命中该阶段即算本级成功；Jenkins 上的构建仍在继续，本工具不会中断它"
        >
          按阶段「{stage.passedOnStage}」提前放行
        </span>
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

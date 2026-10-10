import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  isPermissionGranted,
  requestPermission,
} from "@tauri-apps/plugin-notification";

import * as api from "./api";
import { errorText } from "./api";
import { ServicePicker } from "./components/ServicePicker";
import { RemoteServicePicker } from "./components/RemoteServicePicker";
import { EmptyQueue, TaskCard } from "./components/RunBoard";
import { ServiceModal } from "./components/ServiceModal";
import { SettingsPage } from "./components/SettingsPage";
import type {
  AppConfig,
  LogEntry,
  QueueEvent,
  QueueSnapshot,
  Service,
  StageToken,
} from "./types";
import { isTerminal } from "./types";
import "./App.css";

type Page = "publish" | "settings";

export default function App() {
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [tokenStatus, setTokenStatus] = useState<StageToken[]>([]);
  const [page, setPage] = useState<Page>("publish");
  const [selected, setSelected] = useState<string[]>([]);
  const [queue, setQueue] = useState<QueueSnapshot>({
    stages: [],
    tasks: [],
    pendingCount: 0,
    runningCount: 0,
    successCount: 0,
    failedCount: 0,
  });
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [starting, setStarting] = useState(false);
  const [taskBusy, setTaskBusy] = useState(false);
  const [editing, setEditing] = useState<Service | null>(null);
  const [modalOpen, setModalOpen] = useState(false);
  const [remoteOpen, setRemoteOpen] = useState(false);
  const [autoScroll, setAutoScroll] = useState(true);
  /** 用户手动关掉过的队列汇总签名，避免关掉又立刻弹回来。 */
  const [summaryDismissed, setSummaryDismissed] = useState<string | null>(null);

  /** 已处理的最大事件序号，防止乱序事件把状态回退。 */
  const lastSeq = useRef(0);
  const logBox = useRef<HTMLDivElement | null>(null);

  const running = queue.runningCount + queue.pendingCount > 0;

  // ---- 初始化 ----
  const reload = useCallback(async () => {
    try {
      const [nextConfig, tokens, snapshot] = await Promise.all([
        api.getConfig(),
        api.tokenStatus(),
        api.queueStatus(),
      ]);
      setConfig(nextConfig);
      setTokenStatus(tokens);
      // 上次没跑完的队列也恢复出来，方便接着看。
      setQueue(snapshot);
      setSelected((prev) => {
        if (prev.length > 0) return prev;
        const existing = new Set(nextConfig.services.map((s) => s.name));
        const remembered = nextConfig.lastSelection.filter((n) => existing.has(n));
        if (remembered.length > 0) return remembered;
        return nextConfig.services.filter((s) => s.favorite).map((s) => s.name);
      });
    } catch (e) {
      setError(errorText(e));
    }
  }, []);

  useEffect(() => {
    void reload();
  }, [reload]);

  // 系统通知权限：拿不到也不影响流程，后端仍会尝试发送。
  useEffect(() => {
    (async () => {
      try {
        if (!(await isPermissionGranted())) {
          await requestPermission();
        }
      } catch {
        // 忽略：某些环境没有通知插件实现。
      }
    })();
  }, []);

  // ---- 监听后端推来的队列事件 ----
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;

    listen<QueueEvent>("queue-event", (event) => {
      const payload = event.payload;
      // 事件自带完整快照，按 seq 去重即可（乱序的旧事件直接丢掉）。
      if (payload.seq <= lastSeq.current) return;
      lastSeq.current = payload.seq;

      setQueue(payload.queue);
      if (payload.logs.length > 0) {
        setLogs((prev) => [...prev, ...payload.logs]);
      }
    })
      .then((fn) => {
        if (disposed) fn();
        else unlisten = fn;
      })
      .catch((e) => setError(errorText(e)));

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  // 窗口重新获得焦点时补一次状态，防止最小化期间漏掉事件。
  useEffect(() => {
    function onFocus() {
      api
        .queueStatus()
        .then((snapshot) => {
          // 只在确实更新时替换，避免把刚收到的事件快照冲掉。
          setQueue((prev) =>
            snapshot.tasks.length >= prev.tasks.length ? snapshot : prev,
          );
        })
        .catch(() => undefined);
    }
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, []);

  // 日志自动滚到底部（用户手动往上翻时不打扰）。
  useEffect(() => {
    if (!autoScroll) return;
    const box = logBox.current;
    if (box) box.scrollTop = box.scrollHeight;
  }, [logs, autoScroll]);

  // ---- 派生数据 ----
  const services = config?.services ?? [];
  const stages = config?.stages ?? [];
  /** 环境链展示，例如 `dev → sit → prod`。 */
  const stageChain = stages.map((s) => s.name).join(" → ");

  /** 还缺哪些配置。任一环境没配全就不能发布。 */
  const readiness = useMemo(() => {
    if (!config) return [] as string[];
    const missing: string[] = [];
    if (config.stages.length === 0) {
      missing.push("至少一个流水线环境");
      return missing;
    }
    for (const stage of config.stages) {
      const label = stage.name || "（未命名环境）";
      if (!stage.baseUrl.trim()) missing.push(`${label} 的 Jenkins 地址`);
      if (!stage.username.trim()) missing.push(`${label} 的用户名`);
      if (!tokenStatus.find((t) => t.name === stage.name)?.hasToken) {
        missing.push(`${label} 的 API Token`);
      }
    }
    return missing;
  }, [config, tokenStatus]);

  const finished = !running && queue.tasks.length > 0;
  /** 用「任务数 + 成功数」当签名：队列再次变化时汇总条会重新出现。 */
  const queueSuccessSignature = `${queue.tasks.length}:${queue.successCount}:${queue.failedCount}`;

  // ---- 操作 ----
  function toggleService(name: string) {
    setSelected((prev) =>
      prev.includes(name) ? prev.filter((n) => n !== name) : [...prev, name],
    );
  }

  /** 把选中的服务加入队列。已入队的服务不受影响，可以重复入队。 */
  async function handleEnqueue() {
    if (selected.length === 0) {
      setError("请先勾选要发布的服务");
      return;
    }
    setError(null);
    setNotice(null);
    setStarting(true);
    try {
      const ids = await api.enqueueTasks(selected);
      await api.saveSelection(selected);
      setNotice(`已加入队列 ${ids.length} 个任务，正在并行执行`);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setStarting(false);
    }
  }

  /** 取消单个任务，其他任务继续跑。 */
  async function handleCancelTask(taskId: string) {
    setTaskBusy(true);
    try {
      await api.cancelTask(taskId);
      setNotice("已请求取消该任务，其他任务不受影响");
    } catch (e) {
      setError(errorText(e));
    } finally {
      setTaskBusy(false);
    }
  }

  async function handleResend(taskId: string) {
    setTaskBusy(true);
    try {
      await api.resendNotification(taskId);
      setNotice("已重新发送该任务的通知");
    } catch (e) {
      setError(errorText(e));
    } finally {
      setTaskBusy(false);
    }
  }

  async function handleClearFinished() {
    setTaskBusy(true);
    try {
      const removed = await api.clearFinishedTasks();
      setNotice(removed > 0 ? `已清掉 ${removed} 个已结束的任务` : "没有可清理的任务");
      setQueue(await api.queueStatus());
    } catch (e) {
      setError(errorText(e));
    } finally {
      setTaskBusy(false);
    }
  }

  async function handleToggleFavorite(name: string) {
    try {
      setConfig(await api.toggleFavorite(name));
    } catch (e) {
      setError(errorText(e));
    }
  }

  async function handleDelete(name: string) {
    if (!window.confirm(`确定删除服务「${name}」吗？`)) return;
    try {
      setConfig(await api.deleteService(name));
      setSelected((prev) => prev.filter((n) => n !== name));
    } catch (e) {
      setError(errorText(e));
    }
  }

  async function handleAdd(names: string[]) {
    try {
      setConfig(await api.addServices(names));
    } catch (e) {
      setError(errorText(e));
    }
  }

  async function handleSubmitService(service: Service, originalName?: string) {
    try {
      const updated = await api.saveService(service, originalName);
      setConfig(updated);
      // 重命名后同步勾选状态。
      if (originalName && originalName !== service.name) {
        setSelected((prev) =>
          prev.map((n) => (n === originalName ? service.name : n)),
        );
      }
      setModalOpen(false);
      setEditing(null);
    } catch (e) {
      setError(errorText(e));
    }
  }

  if (!config) {
    return (
      <div className="app-loading">
        <p>正在加载配置…</p>
        {error && <p className="alert alert-error">{error}</p>}
      </div>
    );
  }

  return (
    <div className="app">
      <header className="topbar">
        <div className="brand">
          <span className="brand-mark">JB</span>
          <div>
            <h1>Jenkins 发布管道</h1>
            <p className="muted">
              {stages.length > 0 ? stageChain : "还没配置流水线环境"}
            </p>
          </div>
        </div>
        <nav className="tabs">
          <button
            className={`tab${page === "publish" ? " is-active" : ""}`}
            onClick={() => setPage("publish")}
          >
            发布
          </button>
          <button
            className={`tab${page === "settings" ? " is-active" : ""}`}
            onClick={() => setPage("settings")}
          >
            设置
            {readiness.length > 0 && <span className="dot-warn" title="还有配置未完成" />}
          </button>
        </nav>
      </header>

      {error && (
        <div className="alert alert-error">
          <span>{error}</span>
          <button className="icon-btn" onClick={() => setError(null)}>
            ✕
          </button>
        </div>
      )}
      {notice && (
        <div className="alert alert-info">
          <span>{notice}</span>
          <button className="icon-btn" onClick={() => setNotice(null)}>
            ✕
          </button>
        </div>
      )}
      {readiness.length > 0 && page === "publish" && (
        <div className="alert alert-warn">
          <span>
            还缺少配置：{readiness.join("、")}。
            <button className="link" onClick={() => setPage("settings")}>
              去设置
            </button>
          </span>
        </div>
      )}

      {page === "settings" ? (
        <SettingsPage
          config={config}
          tokenStatus={tokenStatus}
          onConfigChange={setConfig}
          onTokenStatusChange={setTokenStatus}
          onError={setError}
        />
      ) : (
        <main className="publish">
          <div className="publish-left">
            <ServicePicker
              services={services}
              selected={selected}
              locked={false}
              onToggle={toggleService}
              onToggleFavorite={handleToggleFavorite}
              onEdit={(service) => {
                setEditing(service);
                setModalOpen(true);
              }}
              onDelete={handleDelete}
              onSelectAll={(names) =>
                setSelected((prev) => Array.from(new Set([...prev, ...names])))
              }
              onClear={() => setSelected([])}
              onAdd={handleAdd}
              onBrowse={() => setRemoteOpen(true)}
            />

            <section className="panel start-panel">
              <div className="start-row">
                <button
                  className="btn btn-primary btn-lg"
                  onClick={handleEnqueue}
                  disabled={starting || selected.length === 0}
                >
                  {starting ? "加入中…" : `加入队列（${selected.length}）`}
                </button>
              </div>
              <p className="muted small">
                每个服务是一个独立任务，入队后<b>全部并行</b>执行
                {stages.length > 0 ? `（${stageChain}）` : ""}，
                上一级成功才进入下一级；可单独取消某个任务，不影响其他任务。
                每个任务跑完会单独弹一次通知。
              </p>
            </section>
          </div>

          <div className="publish-right">
            <section className="panel board">
              <header className="panel-head">
                <h2>发布队列</h2>
                <div className="board-head-right">
                  <span className="muted small">
                    {queue.runningCount + queue.pendingCount > 0
                      ? `${queue.runningCount} 执行中 / ${queue.pendingCount} 等待`
                      : queue.tasks.length > 0
                        ? "全部结束"
                        : "空"}
                    {queue.tasks.length > 0 &&
                      ` · ${queue.successCount} 成功 / ${queue.failedCount} 失败`}
                  </span>
                  {queue.tasks.some((t) => isTerminal(t.status)) && (
                    <button
                      className="btn btn-ghost"
                      onClick={handleClearFinished}
                      disabled={taskBusy}
                      title="清掉已结束的任务，便于继续用"
                    >
                      清理已结束
                    </button>
                  )}
                </div>
              </header>

              {queue.tasks.length === 0 ? (
                <EmptyQueue hasSelection={selected.length > 0} />
              ) : (
                <div className="cards">
                  {queue.tasks.map((task) => (
                    <TaskCard
                      key={task.id}
                      task={task}
                      onCancel={handleCancelTask}
                      onResend={handleResend}
                      busy={taskBusy}
                    />
                  ))}
                </div>
              )}
            </section>

            <section className="panel logs">
              <header className="panel-head">
                <h2>日志</h2>
                <label className="checkbox small">
                  <input
                    type="checkbox"
                    checked={autoScroll}
                    onChange={(e) => setAutoScroll(e.currentTarget.checked)}
                  />
                  <span>自动滚动</span>
                </label>
              </header>
              <div className="log-box" ref={logBox}>
                {logs.length === 0 && <p className="muted">暂无日志。</p>}
                {logs.map((entry, index) => (
                  <div className={`log-line log-${entry.level}`} key={index}>
                    <span className="log-time">{entry.time}</span>
                    <span className="log-msg">{entry.message}</span>
                  </div>
                ))}
              </div>
            </section>
          </div>
        </main>
      )}

      {/* 队列汇总：通知是按服务发的，这里只做一个窗口内的总体回顾 */}
      {finished && summaryDismissed !== queueSuccessSignature && (
        <div
          className={`result-bar ${
            queue.failedCount > 0 ? "is-bad" : "is-ok"
          }`}
        >
          <div className="result-text">
            <strong>
              {queue.failedCount > 0
                ? `队列已完成，${queue.failedCount} 个任务失败`
                : "队列已全部完成"}
            </strong>
            <span>
              共 {queue.tasks.length} 个任务：{queue.successCount} 成功 /{" "}
              {queue.failedCount} 失败
            </span>
          </div>
          <div className="result-actions">
            <button
              className="btn btn-ghost"
              onClick={handleClearFinished}
              disabled={taskBusy}
            >
              清理已结束
            </button>
            <button
              className="icon-btn"
              onClick={() => setSummaryDismissed(queueSuccessSignature)}
              title="关闭"
            >
              ✕
            </button>
          </div>
        </div>
      )}

      {modalOpen && (
        <ServiceModal
          service={editing}
          onCancel={() => {
            setModalOpen(false);
            setEditing(null);
          }}
          onSubmit={handleSubmitService}
        />
      )}

      {remoteOpen && (
        <RemoteServicePicker
          // 服务清单按「各环境同名 Job」维护，所以列第一个环境的 Job 就够。
          stage={stages[0]?.name}
          stageLabel={stages[0]?.name ?? "第一个环境"}
          existing={services.map((s) => s.name)}
          onAdd={async (names) => {
            // 复用批量添加：已存在的会自动跳过。
            const updated = await api.addServices(names);
            setConfig(updated);
            // 刚添加的服务直接勾上，省一步操作。
            setSelected((prev) => Array.from(new Set([...prev, ...names])));
          }}
          onClose={() => setRemoteOpen(false)}
        />
      )}
    </div>
  );
}

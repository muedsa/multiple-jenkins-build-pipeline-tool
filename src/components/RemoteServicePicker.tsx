import { useEffect, useMemo, useRef, useState } from "react";
import * as api from "../api";
import { errorText } from "../api";
import type { RemoteJob } from "../types";

interface Props {
  /** 从哪个环境拉取 Job 列表；不传就用流水线的第一个环境。 */
  stage?: string;
  /** 环境展示名，出现在标题与提示里。 */
  stageLabel: string;
  /** 服务清单里已经有的名字，用来标出「已添加」。 */
  existing: string[];
  onAdd: (names: string[]) => Promise<void>;
  onClose: () => void;
}

/** 一次最多渲染多少个结果，避免列表过长拖慢界面。 */
const MAX_VISIBLE = 200;

/**
 * 从 Jenkins 远程拉取 Job 列表来搜索添加服务，替代手输服务名。
 *
 * 列表只在打开时拉一次并缓存在组件里，之后的搜索都是本地过滤，
 * 不会每敲一个字就打一次 Jenkins。
 */
export function RemoteServicePicker({
  stage,
  stageLabel,
  existing,
  onAdd,
  onClose,
}: Props) {
  const [jobs, setJobs] = useState<RemoteJob[] | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [keyword, setKeyword] = useState("");
  const [picked, setPicked] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const inputRef = useRef<HTMLInputElement | null>(null);

  const existingSet = useMemo(() => new Set(existing), [existing]);

  // 打开时拉一次列表。
  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    api
      .listRemoteJobs(stage)
      .then((list) => {
        if (!cancelled) {
          setJobs(list);
          setError(null);
        }
      })
      .catch((e) => {
        if (!cancelled) setError(errorText(e));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [stage]);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  // 本地过滤：先按关键字筛，再把已添加的排到后面。
  const { visible, matchedCount, hiddenCount } = useMemo(() => {
    const all = jobs ?? [];
    const kw = keyword.trim().toLowerCase();
    const matched = kw
      ? all.filter(
          (j) =>
            j.name.toLowerCase().includes(kw) ||
            j.displayName.toLowerCase().includes(kw),
        )
      : all;
    const fresh = matched.filter((j) => !existingSet.has(j.name));
    const already = matched.filter((j) => existingSet.has(j.name));
    const ordered = [...fresh, ...already];
    return {
      visible: ordered.slice(0, MAX_VISIBLE),
      matchedCount: matched.length,
      hiddenCount: Math.max(0, ordered.length - MAX_VISIBLE),
    };
  }, [jobs, keyword, existingSet]);

  function toggle(name: string) {
    if (existingSet.has(name)) return;
    setPicked((prev) =>
      prev.includes(name) ? prev.filter((n) => n !== name) : [...prev, name],
    );
  }

  async function handleConfirm() {
    if (picked.length === 0) return;
    setBusy(true);
    try {
      await onAdd(picked);
      onClose();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="modal-mask" onClick={onClose}>
      <div
        className="modal modal-wide"
        onClick={(e) => e.stopPropagation()}
        role="dialog"
        aria-label={`从 ${stageLabel} 添加服务`}
      >
        <header className="modal-head">
          <div>
            <h2>从 {stageLabel} 添加服务</h2>
            <p className="muted small">
              {loading
                ? "正在读取 Jenkins 上的 Job 列表…"
                : jobs
                  ? `共 ${jobs.length} 个可构建 Job`
                  : "读取失败"}
              {picked.length > 0 && ` · 已选 ${picked.length} 个`}
            </p>
          </div>
          <button className="icon-btn" onClick={onClose} title="关闭">
            ✕
          </button>
        </header>

        <input
          ref={inputRef}
          className="input"
          placeholder="搜索 Job 名，支持文件夹层级，如 team-a/svc"
          value={keyword}
          onChange={(e) => setKeyword(e.currentTarget.value)}
          disabled={loading || !!error}
        />

        {error && (
          <div className="alert alert-error modal-alert">
            <span>{error}</span>
          </div>
        )}

        {loading && <p className="empty">正在加载…</p>}

        {!loading && !error && (
          <>
            <div className="remote-list">
              {visible.length === 0 && (
                <p className="empty">
                  {keyword.trim()
                    ? `没有匹配「${keyword.trim()}」的 Job。`
                    : "这台 Jenkins 上没有可添加的 Job。"}
                </p>
              )}
              {visible.map((job) => {
                const isExisting = existingSet.has(job.name);
                const isPicked = picked.includes(job.name);
                return (
                  <button
                    key={job.name}
                    type="button"
                    className={`remote-item${isPicked ? " is-picked" : ""}${
                      isExisting ? " is-existing" : ""
                    }`}
                    onClick={() => toggle(job.name)}
                    disabled={isExisting}
                    title={job.name}
                  >
                    <span className="remote-check">
                      {isExisting ? "✓" : isPicked ? "✓" : ""}
                    </span>
                    <span className="remote-name">
                      {job.displayName}
                      {/* 文件夹里的 Job 显示完整路径，便于区分同名服务 */}
                      {job.name !== job.displayName && (
                        <span className="remote-path">{job.name}</span>
                      )}
                    </span>
                    {isExisting && <span className="tag">已添加</span>}
                  </button>
                );
              })}
            </div>
            {!keyword.trim() && hiddenCount > 0 && (
              <p className="muted small">
                还有 {hiddenCount} 个未显示，输入关键字可继续筛选。
              </p>
            )}
            {keyword.trim() && matchedCount > visible.length && (
              <p className="muted small">
                匹配 {matchedCount} 个，仅显示前 {visible.length} 个。
              </p>
            )}
          </>
        )}

        <div className="modal-foot">
          <button className="btn" onClick={onClose} disabled={busy}>
            取消
          </button>
          <button
            className="btn btn-primary"
            onClick={handleConfirm}
            disabled={busy || picked.length === 0}
          >
            添加 {picked.length > 0 ? `${picked.length} 个` : ""}
          </button>
        </div>
      </div>
    </div>
  );
}

import { useState } from "react";
import type { BuildParam, Service } from "../types";

interface Props {
  /** 传入表示编辑，`null` 表示新增。 */
  service: Service | null;
  onCancel: () => void;
  onSubmit: (service: Service, originalName?: string) => Promise<void>;
}

/** 新增/编辑服务的弹窗。可以顺带配置触发时附带的构建参数。 */
export function ServiceModal({ service, onCancel, onSubmit }: Props) {
  const [name, setName] = useState(service?.name ?? "");
  const [favorite, setFavorite] = useState(service?.favorite ?? false);
  const [params, setParams] = useState<BuildParam[]>(service?.params ?? []);
  const [busy, setBusy] = useState(false);

  function patchParam(index: number, part: Partial<BuildParam>) {
    setParams((prev) =>
      prev.map((p, i) => (i === index ? { ...p, ...part } : p)),
    );
  }

  async function handleSubmit() {
    const trimmed = name.trim();
    if (!trimmed) return;
    setBusy(true);
    try {
      await onSubmit(
        {
          name: trimmed,
          favorite,
          // 丢掉完全空白的参数行，避免提交垃圾数据。
          params: params.filter((p) => p.key.trim() !== ""),
        },
        service?.name,
      );
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="modal-mask" onClick={onCancel}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <header className="modal-head">
          <h2>{service ? "编辑服务" : "添加服务"}</h2>
          <button className="icon-btn" onClick={onCancel} title="关闭">
            ✕
          </button>
        </header>

        <label className="field">
          <span>服务名 / Jenkins Job 名</span>
          <input
            className="input"
            value={name}
            autoFocus
            onChange={(e) => setName(e.currentTarget.value)}
            placeholder="order-service"
          />
        </label>

        <label className="checkbox">
          <input
            type="checkbox"
            checked={favorite}
            onChange={(e) => setFavorite(e.currentTarget.checked)}
          />
          <span>标为常用</span>
        </label>

        <div className="params">
          <div className="params-head">
            <span>构建参数（可选）</span>
            <button
              className="btn btn-ghost"
              onClick={() => setParams((prev) => [...prev, { key: "", value: "" }])}
            >
              ＋ 添加参数
            </button>
          </div>
          {params.length === 0 && (
            <p className="muted">
              这个 Job 不需要参数时就留空。需要参数时会用 buildWithParameters 触发。
            </p>
          )}
          {params.map((param, index) => (
            <div className="param-row" key={index}>
              <input
                className="input"
                placeholder="参数名，如 BRANCH"
                value={param.key}
                onChange={(e) => patchParam(index, { key: e.currentTarget.value })}
              />
              <input
                className="input"
                placeholder="参数值，如 main"
                value={param.value}
                onChange={(e) => patchParam(index, { value: e.currentTarget.value })}
              />
              <button
                className="icon-btn danger"
                title="删除该参数"
                onClick={() =>
                  setParams((prev) => prev.filter((_, i) => i !== index))
                }
              >
                ✕
              </button>
            </div>
          ))}
        </div>

        <div className="modal-foot">
          <button className="btn" onClick={onCancel} disabled={busy}>
            取消
          </button>
          <button
            className="btn btn-primary"
            onClick={handleSubmit}
            disabled={busy || !name.trim()}
          >
            {service ? "保存" : "添加"}
          </button>
        </div>
      </div>
    </div>
  );
}

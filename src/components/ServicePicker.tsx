import { useMemo, useState } from "react";
import type { Service } from "../types";

interface Props {
  services: Service[];
  selected: string[];
  /** 正在运行时禁止改选，避免状态和实际执行对不上。 */
  locked: boolean;
  onToggle: (name: string) => void;
  onToggleFavorite: (name: string) => void;
  onEdit: (service: Service) => void;
  onDelete: (name: string) => void;
  onSelectAll: (names: string[]) => void;
  onClear: () => void;
  onAdd: (names: string[]) => Promise<void>;
  /** 打开「从 Jenkins 搜索添加」弹窗。 */
  onBrowse: () => void;
}

/**
 * 服务选择区：支持搜索、只看常用、全选/清空，以及把服务标为常用方便下次快速勾选。
 */
export function ServicePicker({
  services,
  selected,
  locked,
  onToggle,
  onToggleFavorite,
  onEdit,
  onDelete,
  onSelectAll,
  onClear,
  onAdd,
  onBrowse,
}: Props) {
  const [keyword, setKeyword] = useState("");
  const [favoritesOnly, setFavoritesOnly] = useState(false);
  const [adding, setAdding] = useState(false);
  const [draft, setDraft] = useState("");

  const selectedSet = useMemo(() => new Set(selected), [selected]);

  const visible = useMemo(() => {
    const kw = keyword.trim().toLowerCase();
    return services.filter((s) => {
      if (favoritesOnly && !s.favorite) return false;
      if (kw && !s.name.toLowerCase().includes(kw)) return false;
      return true;
    });
  }, [services, keyword, favoritesOnly]);

  const favoriteCount = services.filter((s) => s.favorite).length;

  /** 批量添加：按行/逗号/空格拆分成多个服务名。 */
  async function handleAdd() {
    const names = draft
      .split(/[\n,，;；\s]+/)
      .map((n) => n.trim())
      .filter(Boolean);
    if (names.length === 0) return;
    await onAdd(names);
    setDraft("");
    setAdding(false);
  }

  return (
    <section className="panel picker">
      <header className="panel-head">
        <h2>选择服务</h2>
        <span className="muted">
          已选 {selected.length} / {services.length}
        </span>
      </header>

      <div className="picker-tools">
        <input
          className="input"
          placeholder="搜索服务名…"
          value={keyword}
          onChange={(e) => setKeyword(e.currentTarget.value)}
          disabled={locked}
        />
        <label className="checkbox">
          <input
            type="checkbox"
            checked={favoritesOnly}
            onChange={(e) => setFavoritesOnly(e.currentTarget.checked)}
            disabled={locked}
          />
          <span>只看常用 ({favoriteCount})</span>
        </label>
      </div>

      <div className="picker-actions">
        <button
          className="btn btn-ghost"
          onClick={() => onSelectAll(visible.map((s) => s.name))}
          disabled={locked || visible.length === 0}
        >
          全选当前
        </button>
        <button
          className="btn btn-ghost"
          onClick={onClear}
          disabled={locked || selected.length === 0}
        >
          清空
        </button>
        <button
          className="btn btn-ghost"
          onClick={onBrowse}
          disabled={locked}
        >
          ☁ 从 Jenkins 搜索
        </button>
        <button
          className="btn btn-ghost"
          onClick={() => setAdding((v) => !v)}
          disabled={locked}
        >
          {adding ? "取消" : "＋ 手动输入"}
        </button>
      </div>

      {adding && (
        <div className="add-box">
          <div className="add-col">
            <textarea
              className="input"
              rows={3}
              placeholder={"每行一个服务名，也可用逗号分隔\norder-service\nuser-service"}
              value={draft}
              onChange={(e) => setDraft(e.currentTarget.value)}
            />
            <p className="muted small">
              手输容易把 Job 名写错；用「☁ 从 Jenkins 搜索」能直接选到真实存在的 Job。
            </p>
          </div>
          <button className="btn btn-primary" onClick={handleAdd} disabled={!draft.trim()}>
            添加
          </button>
        </div>
      )}

      <div className="service-list">
        {visible.length === 0 && (
          <p className="empty">
            {services.length === 0
              ? "还没有服务，先点「＋ 添加服务」把要发布的 Job 名加进来。"
              : "没有匹配的服务。"}
          </p>
        )}
        {visible.map((service) => {
          const isSelected = selectedSet.has(service.name);
          return (
            <div
              key={service.name}
              className={`service-item${isSelected ? " is-selected" : ""}`}
            >
              <label className="service-main">
                <input
                  type="checkbox"
                  checked={isSelected}
                  onChange={() => onToggle(service.name)}
                  disabled={locked}
                />
                <span className="service-name" title={service.name}>
                  {service.name}
                </span>
                {service.params.length > 0 && (
                  <span className="tag" title="触发时会带上这些参数">
                    参数 {service.params.length}
                  </span>
                )}
              </label>
              <div className="service-side">
                <button
                  className={`icon-btn${service.favorite ? " is-on" : ""}`}
                  title={service.favorite ? "取消常用" : "标为常用"}
                  onClick={() => onToggleFavorite(service.name)}
                  disabled={locked}
                >
                  ★
                </button>
                <button
                  className="icon-btn"
                  title="编辑"
                  onClick={() => onEdit(service)}
                  disabled={locked}
                >
                  ✎
                </button>
                <button
                  className="icon-btn danger"
                  title="删除"
                  onClick={() => onDelete(service.name)}
                  disabled={locked}
                >
                  ✕
                </button>
              </div>
            </div>
          );
        })}
      </div>
    </section>
  );
}

import { useEffect, useState } from "react";
import * as api from "../api";
import { errorText } from "../api";
import type {
  AppConfig,
  PreviewContent,
  ProbeResult,
  StageConfig,
  StageToken,
  TemplateConfig,
  TemplateVariable,
} from "../types";

interface Props {
  config: AppConfig;
  tokenStatus: StageToken[];
  onConfigChange: (config: AppConfig) => void;
  onTokenStatusChange: (status: StageToken[]) => void;
  onError: (message: string) => void;
}

/** 某个环境是否已经存好 token。 */
function hasToken(status: StageToken[], name: string): boolean {
  return status.find((s) => s.name === name)?.hasToken ?? false;
}

/** 新建环境时的默认值。 */
function defaultStage(name: string): StageConfig {
  return {
    name,
    baseUrl: "",
    username: "",
    insecureTls: false,
    pollIntervalSecs: 5,
    timeoutMinutes: 90,
    abortOnFailure: false,
    passOnStage: "",
  };
}

/** 新建环境时的默认名字：stage-1、stage-2 … 避开已有的。 */
function nextStageName(existing: string[]): string {
  for (let i = 1; i < 1000; i += 1) {
    const candidate = `stage-${i}`;
    if (!existing.includes(candidate)) return candidate;
  }
  return `stage-${Date.now()}`;
}

/**
 * 单个环境的编辑表单。默认折叠，点标题栏展开 ——
 * 环境多了以后不至于整页都是输入框。
 */
function StageForm({
  stage,
  index,
  total,
  storedToken,
  onSave,
  onDelete,
  onMove,
  onTokenCleared,
  onError,
}: {
  stage: StageConfig;
  index: number;
  total: number;
  /** 该环境在凭据管理器里是否已有 token。 */
  storedToken: boolean;
  onSave: (stage: StageConfig) => Promise<void>;
  onDelete: () => Promise<void>;
  onMove: (direction: -1 | 1) => Promise<void>;
  onTokenCleared: () => Promise<void>;
  onError: (message: string) => void;
}) {
  const [draft, setDraft] = useState<StageConfig>(stage);
  const [tokenInput, setTokenInput] = useState("");
  const [busy, setBusy] = useState(false);
  const [probe, setProbe] = useState<ProbeResult | null>(null);
  const [hint, setHint] = useState<string | null>(null);
  const [open, setOpen] = useState(false);

  // 父组件刷新配置后同步一次，避免表单里停留着旧值。
  useEffect(() => {
    setDraft(stage);
    setHint(null);
  }, [stage]);

  function patch(part: Partial<StageConfig>) {
    setDraft((prev) => ({ ...prev, ...part }));
    setHint(null);
  }

  async function handleSave() {
    setBusy(true);
    setHint(null);
    try {
      const name = draft.name.trim();
      if (!name) {
        onError("环境名不能为空");
        return;
      }
      // 先存 token（如果填了），再存环境本身。
      if (tokenInput.trim()) {
        await api.saveToken(name, tokenInput.trim());
        setTokenInput("");
        await onTokenCleared();
      }
      await onSave({ ...draft, name });
      setHint("已保存");
    } catch (e) {
      onError(errorText(e));
    } finally {
      setBusy(false);
    }
  }

  async function handleClearToken() {
    setBusy(true);
    try {
      await api.clearToken(draft.name);
      await onTokenCleared();
      setHint("Token 已清除");
    } catch (e) {
      onError(errorText(e));
    } finally {
      setBusy(false);
    }
  }

  async function handleTest() {
    setBusy(true);
    setProbe(null);
    try {
      const result = await api.testConnection({
        ...draft,
        name: draft.name.trim(),
        baseUrl: draft.baseUrl.trim(),
        username: draft.username.trim(),
      });
      setProbe(result);
    } catch (e) {
      setProbe({ ok: false, message: errorText(e), version: null, mode: null });
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className={`stage-card${open ? " is-open" : ""}`}>
      <header className="stage-head">
        <button
          className="stage-toggle"
          onClick={() => setOpen((v) => !v)}
          title={open ? "收起" : "展开编辑"}
        >
          <span className="stage-order">{index + 1}</span>
          <span className="stage-name">{draft.name || "（未命名）"}</span>
          <span className="stage-url">{draft.baseUrl || "未配置地址"}</span>
        </button>
        <div className="stage-badges">
          {draft.passOnStage.trim() && (
            <span
              className="pill"
              title={`阶段「${draft.passOnStage.trim()}」一成功，本级就算通过并进入下一环境`}
            >
              阶段放行：{draft.passOnStage.trim()}
            </span>
          )}
          {draft.abortOnFailure && (
            <span className="pill pill-warn" title="本级失败后中止整条流水线">
              失败即中止
            </span>
          )}
          <span
            className={storedToken ? "pill pill-ok" : "pill pill-warn"}
            title={storedToken ? "凭据管理器里已有该环境的 Token" : "还没保存 Token"}
          >
            {storedToken ? "已存 Token" : "未存 Token"}
          </span>
        </div>
        <div className="stage-actions">
          <button
            className="icon-btn"
            title="上移一级"
            disabled={busy || index === 0}
            onClick={() => onMove(-1)}
          >
            ↑
          </button>
          <button
            className="icon-btn"
            title="下移一级"
            disabled={busy || index === total - 1}
            onClick={() => onMove(1)}
          >
            ↓
          </button>
          <button
            className="icon-btn danger"
            title="删除该环境"
            disabled={busy}
            onClick={onDelete}
          >
            ✕
          </button>
        </div>
      </header>

      {open && (
        <div className="stage-body">
          <div className="form-grid">
            <label className="field">
              <span>环境名（同时作为凭据名）</span>
              <input
                className="input"
                value={draft.name}
                onChange={(e) => patch({ name: e.currentTarget.value })}
                placeholder="dev / sit / uat / prod"
              />
            </label>
            <label className="field">
              <span>Jenkins 地址</span>
              <input
                className="input"
                value={draft.baseUrl}
                onChange={(e) => patch({ baseUrl: e.currentTarget.value })}
                placeholder="http://jenkins-dev.internal:8080"
              />
            </label>
            <label className="field">
              <span>用户名</span>
              <input
                className="input"
                value={draft.username}
                onChange={(e) => patch({ username: e.currentTarget.value })}
                placeholder="Jenkins 登录账号"
              />
            </label>
            <label className="field">
              <span>API Token</span>
              <input
                className="input"
                type="password"
                value={tokenInput}
                onChange={(e) => setTokenInput(e.currentTarget.value)}
                placeholder={
                  storedToken ? "已保存，留空则不修改" : "粘贴 Jenkins API Token"
                }
                autoComplete="off"
              />
            </label>
            <label className="field">
              <span>轮询间隔（秒）</span>
              <input
                className="input"
                type="number"
                min={1}
                value={draft.pollIntervalSecs}
                onChange={(e) =>
                  patch({ pollIntervalSecs: Number(e.currentTarget.value) || 5 })
                }
              />
            </label>
            <label className="field">
              <span>单次构建超时（分钟）</span>
              <input
                className="input"
                type="number"
                min={1}
                value={draft.timeoutMinutes}
                onChange={(e) =>
                  patch({ timeoutMinutes: Number(e.currentTarget.value) || 90 })
                }
              />
            </label>
            <label className="field field-wide">
              <span>命中此阶段即算本级成功（忽略后续 stage）</span>
              <input
                className="input"
                value={draft.passOnStage}
                onChange={(e) => patch({ passOnStage: e.currentTarget.value })}
                placeholder="留空 = 按整条构建的结果判定；例如 Deploy"
              />
              <span className="field-hint">
                填 Jenkins Pipeline 里某个 stage 的名字（不区分大小写）。它一成功，本级就算通过，
                直接开始下一个环境的构建，不等后面的 stage 跑完。
                Jenkins 上正在跑的构建<strong>不会被中断</strong>。
                阶段信息来自 Pipeline Stage View 插件，取不到时自动退回按整条构建结果判定。
              </span>
            </label>
          </div>

          <label className="checkbox">
            <input
              type="checkbox"
              checked={draft.insecureTls}
              onChange={(e) => patch({ insecureTls: e.currentTarget.checked })}
            />
            <span>忽略 TLS 证书校验（仅在内网使用自签名证书时打开）</span>
          </label>

          <label className="checkbox">
            <input
              type="checkbox"
              checked={draft.abortOnFailure}
              onChange={(e) => patch({ abortOnFailure: e.currentTarget.checked })}
            />
            <span>
              本级失败后中止整条流水线（不勾选时，后面环境会继续发布）
            </span>
          </label>

          <div className="row-actions">
            <button className="btn btn-primary" onClick={handleSave} disabled={busy}>
              保存
            </button>
            <button className="btn" onClick={handleTest} disabled={busy}>
              测试连接
            </button>
            {storedToken && (
              <button
                className="btn btn-ghost danger"
                onClick={handleClearToken}
                disabled={busy}
              >
                清除 Token
              </button>
            )}
            {hint && <span className="muted">{hint}</span>}
          </div>

          {probe && (
            <p className={`probe ${probe.ok ? "probe-ok" : "probe-bad"}`}>
              {probe.message}
            </p>
          )}
        </div>
      )}
    </section>
  );
}

/** 流水线环境列表：增删、排序、逐个编辑。 */
function StageList({
  config,
  tokenStatus,
  onConfigChange,
  onTokenStatusChange,
  onError,
}: Props) {
  const stages = config.stages;

  async function refreshTokens() {
    try {
      onTokenStatusChange(await api.tokenStatus());
    } catch {
      // 凭据库读不到不算致命，忽略。
    }
  }

  async function handleAdd() {
    try {
      const stage = defaultStage(nextStageName(stages.map((s) => s.name)));
      onConfigChange(await api.saveStage(stage));
    } catch (e) {
      onError(errorText(e));
    }
  }

  async function handleSave(stage: StageConfig, originalName: string) {
    // 改名时后端会把 token 一起挪过去。
    onConfigChange(await api.saveStage(stage, originalName));
    await refreshTokens();
  }

  async function handleDelete(name: string) {
    if (
      !window.confirm(`确定删除环境「${name}」吗？该环境的配置会一并移除。`)
    ) {
      return;
    }
    try {
      onConfigChange(await api.deleteStage(name));
      await refreshTokens();
    } catch (e) {
      onError(errorText(e));
    }
  }

  async function handleMove(index: number, direction: -1 | 1) {
    const target = index + direction;
    if (target < 0 || target >= stages.length) return;
    const names = stages.map((s) => s.name);
    [names[index], names[target]] = [names[target], names[index]];
    try {
      onConfigChange(await api.reorderStages(names));
    } catch (e) {
      onError(errorText(e));
    }
  }

  return (
    <section className="panel">
      <header className="panel-head">
        <h2>流水线环境</h2>
        <span className="muted">按从上到下的顺序依次发布</span>
      </header>

      <p className="stage-chain-hint">
        {stages.length === 0
          ? "还没有环境，点下方「添加环境」开始。"
          : stages.map((s) => s.name).join("  →  ")}
      </p>

      <div className="stage-list">
        {stages.map((stage, index) => (
          <StageForm
            key={stage.name}
            stage={stage}
            index={index}
            total={stages.length}
            storedToken={hasToken(tokenStatus, stage.name)}
            onSave={(next) => handleSave(next, stage.name)}
            onDelete={() => handleDelete(stage.name)}
            onMove={(dir) => handleMove(index, dir)}
            onTokenCleared={refreshTokens}
            onError={onError}
          />
        ))}
      </div>

      <div className="row-actions">
        <button className="btn btn-ghost" onClick={handleAdd}>
          ＋ 添加环境
        </button>
      </div>
    </section>
  );
}

/** 通知文案模板编辑器，带实时预览。 */
function TemplateForm({
  template,
  onSaved,
  onError,
}: {
  template: TemplateConfig;
  onSaved: (config: AppConfig) => void;
  onError: (message: string) => void;
}) {
  const [draft, setDraft] = useState<TemplateConfig>(template);
  const [preview, setPreview] = useState<PreviewContent | null>(null);
  const [variables, setVariables] = useState<TemplateVariable[]>([]);
  const [saved, setSaved] = useState(false);
  const [busy, setBusy] = useState(false);

  useEffect(() => setDraft(template), [template]);

  useEffect(() => {
    api.templateVariables().then(setVariables).catch(() => setVariables([]));
  }, []);

  // 每次改动都重新渲染预览，让用户立刻看到效果。
  useEffect(() => {
    let cancelled = false;
    const timer = window.setTimeout(() => {
      api
        .previewTemplate(draft)
        .then((result) => {
          if (!cancelled) setPreview(result);
        })
        .catch(() => {
          if (!cancelled) setPreview(null);
        });
    }, 150);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [draft]);

  function patch(part: Partial<TemplateConfig>) {
    setDraft((prev) => ({ ...prev, ...part }));
    setSaved(false);
  }

  /** 点击变量就插到正文末尾。 */
  function insertVariable(key: string) {
    const placeholder = `{{${key}}}`;
    setDraft((prev) => ({ ...prev, body: `${prev.body}${placeholder}` }));
    setSaved(false);
  }

  async function handleSave() {
    setBusy(true);
    try {
      onSaved(await api.saveTemplate(draft));
      setSaved(true);
    } catch (e) {
      onError(errorText(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="panel">
      <header className="panel-head">
        <h2>完成通知文案</h2>
        <span className="muted">支持 {'{{变量}}'} 占位符</span>
      </header>

      <div className="var-chips">
        {variables.map((v) => (
          <button
            key={v.key}
            className="chip"
            title={v.description}
            onClick={() => insertVariable(v.key)}
          >
            {`{{${v.key}}}`}
          </button>
        ))}
      </div>

      <div className="form-grid">
        <label className="field">
          <span>成功标题</span>
          <input
            className="input"
            value={draft.title}
            onChange={(e) => patch({ title: e.currentTarget.value })}
          />
        </label>
        <label className="field field-wide">
          <span>成功内容</span>
          <textarea
            className="input"
            rows={2}
            value={draft.body}
            onChange={(e) => patch({ body: e.currentTarget.value })}
          />
        </label>
      </div>

      <label className="checkbox">
        <input
          type="checkbox"
          checked={draft.useFailureTemplate}
          onChange={(e) => patch({ useFailureTemplate: e.currentTarget.checked })}
        />
        <span>存在失败时使用另一套文案</span>
      </label>

      {draft.useFailureTemplate && (
        <div className="form-grid">
          <label className="field">
            <span>失败标题</span>
            <input
              className="input"
              value={draft.failureTitle}
              onChange={(e) => patch({ failureTitle: e.currentTarget.value })}
            />
          </label>
          <label className="field field-wide">
            <span>失败内容</span>
            <textarea
              className="input"
              rows={2}
              value={draft.failureBody}
              onChange={(e) => patch({ failureBody: e.currentTarget.value })}
            />
          </label>
        </div>
      )}

      <div className="row-actions">
        <button className="btn btn-primary" onClick={handleSave} disabled={busy}>
          保存文案
        </button>
        {saved && <span className="muted">已保存</span>}
      </div>

      {preview && (
        <div className="preview">
          <p className="preview-title">预览（示例数据）</p>
          <div className="toast toast-ok">
            <strong>{preview.successTitle}</strong>
            <span>{preview.successBody}</span>
          </div>
          <div className="toast toast-bad">
            <strong>{preview.failureTitle}</strong>
            <span>{preview.failureBody}</span>
          </div>
        </div>
      )}
    </section>
  );
}

/** 设置页：流水线环境列表 + 通知文案。 */
export function SettingsPage({
  config,
  tokenStatus,
  onConfigChange,
  onTokenStatusChange,
  onError,
}: Props) {
  return (
    <div className="settings">
      <StageList
        config={config}
        tokenStatus={tokenStatus}
        onConfigChange={onConfigChange}
        onTokenStatusChange={onTokenStatusChange}
        onError={onError}
      />
      <TemplateForm
        template={config.template}
        onSaved={onConfigChange}
        onError={onError}
      />

      <section className="panel">
        <header className="panel-head">
          <h2>使用说明</h2>
        </header>
        <ol className="help-list">
          <li>
            在「流水线环境」里按发布顺序添加环境（如 dev → sit → uat → prod）。
            每个环境填好 Jenkins 地址、用户名，并粘贴 <b>API Token </b>后保存。
            Token 会存进 Windows 凭据管理器，不会写进配置文件。
          </li>
          <li>
            用 <b>↑ ↓</b> 调整顺序 —— <b>顺序就是执行顺序</b>。点标题栏展开编辑。
          </li>
          <li>
            每个环境可以单独勾选
            <b>「本级失败后中止整条流水线」</b>。不勾选时，某一级失败只会把该级
            标为失败，后面的环境照样继续发布。
          </li>
          <li>点「测试连接」确认每个环境都能通，并核对 Jenkins 版本。</li>
          <li>
            回到「发布」页添加服务名。约定各环境上是<b>同名 Job</b>
            ，工具会用各自的地址分别触发。
          </li>
          <li>
            勾选要发布的服务（常用的可以点 ★ 标记）后点「开始发布」。
            所有服务并行执行，每个服务自己沿环境链逐级推进；全部结束时弹出系统通知。
          </li>
        </ol>
      </section>
    </div>
  );
}

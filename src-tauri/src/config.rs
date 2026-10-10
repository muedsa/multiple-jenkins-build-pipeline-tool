//! 应用配置：多级流水线的各环境、服务清单、常用服务与通知模板。
//!
//! 配置以 JSON 明文保存（不含 token）；token 单独存放在系统凭据管理器里，
//! 见 [`crate::secret`]。

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// 一个流水线环境（一级）。按数组顺序执行：前一级成功才进入下一级。
///
/// `name` 既是显示名，也是**环境标识**——日志前缀、凭据条目都用它，
/// 所以要非空且互不重复。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StageConfig {
    /// 环境名，如 `dev` / `sit` / `prod`。同时作为凭据管理器里的条目名。
    pub name: String,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub username: String,
    /// 是否关闭 TLS 证书校验（自签名证书的内网 Jenkins 需要）。
    #[serde(default)]
    pub insecure_tls: bool,
    /// 解析 Jenkins 队列项、等待构建结束时的轮询间隔（秒）。
    #[serde(default = "default_poll_interval")]
    pub poll_interval_secs: u64,
    /// 单个构建允许的最长等待时间（分钟），超时判定失败。
    #[serde(default = "default_build_timeout")]
    pub timeout_minutes: u64,
    /// 本级失败后是否中止整条流水线（后面所有级都不再执行）。
    /// 默认 `false`：只把本级标为失败，后续级照样往下推进。
    #[serde(default)]
    pub abort_on_failure: bool,
}

fn default_poll_interval() -> u64 {
    5
}

fn default_build_timeout() -> u64 {
    90
}

impl StageConfig {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            base_url: String::new(),
            username: String::new(),
            insecure_tls: false,
            poll_interval_secs: default_poll_interval(),
            timeout_minutes: default_build_timeout(),
            abort_on_failure: false,
        }
    }

    /// 判断这个环境是否已经填够了必要信息。
    pub fn is_ready(&self) -> bool {
        !self.base_url.trim().is_empty() && !self.username.trim().is_empty()
    }

    /// 把缺失/非法字段补回默认值。
    fn normalize(&mut self) {
        self.name = self.name.trim().to_string();
        self.base_url = self.base_url.trim().to_string();
        self.username = self.username.trim().to_string();
        if self.poll_interval_secs == 0 {
            self.poll_interval_secs = default_poll_interval();
        }
        if self.timeout_minutes == 0 {
            self.timeout_minutes = default_build_timeout();
        }
    }
}

/// 一个可发布的服务。约定所有环境上是同名 Job，只是服务器地址不同。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Service {
    pub name: String,
    /// 标记为常用，便于在列表顶部快速勾选。
    #[serde(default)]
    pub favorite: bool,
    /// 触发构建时附带的参数（可选）。所有环境共用这套参数。
    #[serde(default)]
    pub params: Vec<BuildParam>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildParam {
    pub key: String,
    pub value: String,
}

/// 通知文案模板，支持占位符，见 [`crate::notify::render_template`]。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateConfig {
    #[serde(default = "default_title_template")]
    pub title: String,
    #[serde(default = "default_body_template")]
    pub body: String,
    /// 失败时是否单独用一套文案。
    #[serde(default = "default_true")]
    pub use_failure_template: bool,
    #[serde(default = "default_failure_title")]
    pub failure_title: String,
    #[serde(default = "default_failure_body")]
    pub failure_body: String,
    /// 任务被用户取消时用的文案（和「失败」区分开，避免误以为真挂了）。
    #[serde(default = "default_cancelled_title")]
    pub cancelled_title: String,
    #[serde(default = "default_cancelled_body")]
    pub cancelled_body: String,
}

fn default_true() -> bool {
    true
}

fn default_title_template() -> String {
    "✅ {{serviceName}} 发布完成".to_string()
}

fn default_body_template() -> String {
    "已在 {{stageChain}} 全部发布成功（耗时 {{duration}}）".to_string()
}

fn default_failure_title() -> String {
    "❌ {{serviceName}} 发布失败".to_string()
}

fn default_failure_body() -> String {
    "卡在 {{failedStage}}：{{status}}（链路 {{stageChain}}，耗时 {{duration}}）".to_string()
}

fn default_cancelled_title() -> String {
    "⊘ {{serviceName}} 已取消".to_string()
}

fn default_cancelled_body() -> String {
    "任务被手动取消，停在 {{failedStage}}（耗时 {{duration}}）".to_string()
}

impl Default for TemplateConfig {
    fn default() -> Self {
        Self {
            title: default_title_template(),
            body: default_body_template(),
            use_failure_template: true,
            failure_title: default_failure_title(),
            failure_body: default_failure_body(),
            cancelled_title: default_cancelled_title(),
            cancelled_body: default_cancelled_body(),
        }
    }
}

/// 整体配置。`version` 便于以后做迁移。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    pub version: u32,
    /// 流水线环境，按数组顺序依次执行。
    #[serde(default)]
    pub stages: Vec<StageConfig>,
    #[serde(default)]
    pub services: Vec<Service>,
    #[serde(default)]
    pub template: TemplateConfig,
    /// 最近一次选择的服务名，启动时回填勾选状态。
    #[serde(default)]
    pub last_selection: Vec<String>,
}

/// 当前配置格式版本。v1 是固定的 dev/test 两级；v2 起改为任意级 `stages`。
pub const CONFIG_VERSION: u32 = 2;

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            // 默认给一条 dev → test 的流水线，和旧版行为一致，开箱即用。
            stages: vec![StageConfig::new("dev"), StageConfig::new("test")],
            services: Vec::new(),
            template: TemplateConfig::default(),
            last_selection: Vec::new(),
        }
    }
}

impl AppConfig {
    /// 补齐缺失的字段，保证从旧文件读出来的配置也能用。
    fn normalize(&mut self) {
        if self.version < CONFIG_VERSION {
            self.version = CONFIG_VERSION;
        }
        for stage in &mut self.stages {
            stage.normalize();
        }
        // 环境名是凭据的键，不能为空、也不能重复；重复的只保留第一个。
        let mut seen: Vec<String> = Vec::new();
        self.stages.retain(|s| {
            if s.name.is_empty() || seen.iter().any(|n| n == &s.name) {
                return false;
            }
            seen.push(s.name.clone());
            true
        });

        // 服务名去空白并去重，避免出现两个同名服务互相覆盖状态。
        let mut seen: Vec<String> = Vec::new();
        self.services.retain_mut(|s| {
            let name = s.name.trim().to_string();
            if name.is_empty() || seen.iter().any(|x| x == &name) {
                return false;
            }
            s.name = name;
            seen.push(s.name.clone());
            true
        });
    }

    /// 按名字查找服务。
    pub fn service(&self, name: &str) -> Option<&Service> {
        self.services.iter().find(|s| s.name == name)
    }

    /// 按名字查找环境。
    pub fn stage(&self, name: &str) -> Option<&StageConfig> {
        self.stages.iter().find(|s| s.name == name)
    }

    /// 环境名的顺序，用作流水线链路展示与日志。
    pub fn stage_names(&self) -> Vec<String> {
        self.stages.iter().map(|s| s.name.clone()).collect()
    }
}

/// 从磁盘读到的原始环境配置。
///
/// v1 用的是 `id` + `displayName`，v2 起用 `name`。这里两种都接下来，
/// 再统一转成 [`StageConfig`]。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawStage {
    /// v2 的环境名。
    #[serde(default)]
    name: Option<String>,
    /// v1 的环境标识，迁移时当作 name 用。
    #[serde(default)]
    id: Option<String>,
    /// v1 的显示名，现在已并入 name，读出来只为了兼容旧文件。
    #[serde(default, rename = "displayName")]
    _display_name: Option<String>,
    #[serde(default)]
    base_url: String,
    #[serde(default)]
    username: String,
    #[serde(default)]
    insecure_tls: bool,
    #[serde(default)]
    poll_interval_secs: Option<u64>,
    #[serde(default)]
    timeout_minutes: Option<u64>,
    #[serde(default)]
    abort_on_failure: bool,
}

impl RawStage {
    /// 转成正式配置；`fallback` 是名字都取不到时的兜底。
    fn into_config(self, fallback: Option<&str>) -> StageConfig {
        let name = self
            .name
            .or(self.id)
            .map(|n| n.trim().to_string())
            .filter(|n| !n.is_empty())
            .or_else(|| fallback.map(str::to_string))
            .unwrap_or_default();

        let mut stage = StageConfig::new(&name);
        stage.base_url = self.base_url;
        stage.username = self.username;
        stage.insecure_tls = self.insecure_tls;
        if let Some(v) = self.poll_interval_secs {
            stage.poll_interval_secs = v;
        }
        if let Some(v) = self.timeout_minutes {
            stage.timeout_minutes = v;
        }
        stage.abort_on_failure = self.abort_on_failure;
        stage
    }
}

/// 从磁盘读到的原始配置。
///
/// v1 的格式是固定的 `dev` / `test` 两个对象，v2 起是 `stages` 数组。
/// 这里把两种形状都接下来，再统一转换成 [`AppConfig`]。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawConfig {
    #[serde(default)]
    stages: Option<Vec<RawStage>>,
    /// v1 遗留字段：dev 级。
    #[serde(default)]
    dev: Option<RawStage>,
    /// v1 遗留字段：test 级。
    #[serde(default)]
    test: Option<RawStage>,
    #[serde(default)]
    services: Vec<Service>,
    #[serde(default)]
    template: TemplateConfig,
    #[serde(default)]
    last_selection: Vec<String>,
}

impl RawConfig {
    /// 把 v1 的 dev/test 迁移成 stages。
    ///
    /// 关键点：环境名沿用原来的 `dev` / `test`，因为凭据管理器是按环境名存的，
    /// 换名字会让用户已保存的 API Token 变成“丢失”。
    fn migrate(self) -> AppConfig {
        let stages = match self.stages {
            Some(stages) if !stages.is_empty() => stages
                .into_iter()
                .map(|s| s.into_config(None))
                .collect::<Vec<_>>(),
            _ => {
                let mut stages = Vec::new();
                if let Some(dev) = self.dev {
                    stages.push(dev.into_config(Some("dev")));
                }
                if let Some(test) = self.test {
                    stages.push(test.into_config(Some("test")));
                }
                if stages.is_empty() {
                    // 什么都没配过，给个能直接用的默认流水线。
                    AppConfig::default().stages
                } else {
                    stages
                }
            }
        };

        AppConfig {
            version: CONFIG_VERSION,
            stages,
            services: self.services,
            template: self.template,
            last_selection: self.last_selection,
        }
    }
}

/// 把配置持久化到磁盘，并在内存里保存一份供命令读写。
pub struct ConfigStore {
    path: PathBuf,
    inner: Mutex<AppConfig>,
}

impl ConfigStore {
    /// 从 `dir` 读取 `config.json`；文件缺失或损坏时退回默认配置。
    pub fn load(dir: &Path) -> Self {
        let path = dir.join("config.json");
        let mut config = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str::<RawConfig>(&text).ok())
            .map(RawConfig::migrate)
            .unwrap_or_default();
        config.normalize();
        Self {
            path,
            inner: Mutex::new(config),
        }
    }

    /// 读取一份配置副本。
    pub fn get(&self) -> Result<AppConfig, String> {
        self.inner
            .lock()
            .map(|guard| guard.clone())
            .map_err(|_| "配置读取失败：内部锁已损坏".to_string())
    }

    /// 覆盖整份配置并落盘。
    pub fn replace(&self, mut next: AppConfig) -> Result<AppConfig, String> {
        next.normalize();
        self.write(&next)?;
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| "配置保存失败：内部锁已损坏".to_string())?;
        *guard = next.clone();
        Ok(next)
    }

    /// 在配置上做局部修改。
    pub fn update<F>(&self, edit: F) -> Result<AppConfig, String>
    where
        F: FnOnce(&mut AppConfig),
    {
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| "配置更新失败：内部锁已损坏".to_string())?;
        let mut next = guard.clone();
        edit(&mut next);
        next.normalize();
        self.write(&next)?;
        *guard = next.clone();
        Ok(next)
    }

    fn write(&self, config: &AppConfig) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("创建配置目录失败：{e}"))?;
        }
        let text = serde_json::to_string_pretty(config)
            .map_err(|e| format!("序列化配置失败：{e}"))?;
        // 先写临时文件再替换，避免中途失败留下半个文件。
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, text).map_err(|e| format!("写入配置失败：{e}"))?;
        std::fs::rename(&tmp, &self.path).map_err(|e| format!("保存配置失败：{e}"))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_pipeline_is_dev_then_test() {
        let config = AppConfig::default();
        assert_eq!(config.stage_names(), vec!["dev", "test"]);
        assert!(!config.stages[0].abort_on_failure);
    }

    /// v1（固定 dev/test）要能平滑升级成 v2 的多级 stages，
    /// 并且环境名沿用原来的 dev/test —— 否则凭据管理器里的 token 会“丢失”。
    #[test]
    fn migrates_legacy_dev_test_config() {
        let legacy = r####"{
          "version": 1,
          "dev": { "id": "dev", "displayName": "开发环境 (dev)", "baseUrl": "http://dev:8080",
                   "username": "u1", "insecureTls": false, "pollIntervalSecs": 3, "timeoutMinutes": 30 },
          "test": { "id": "test", "displayName": "测试环境 (test)", "baseUrl": "http://test:8080",
                    "username": "u2", "insecureTls": true, "pollIntervalSecs": 7, "timeoutMinutes": 45 },
          "services": [{ "name": "svc-a", "favorite": true, "params": [] }],
          "lastSelection": ["svc-a"]
        }"####;

        let raw: RawConfig = serde_json::from_str(legacy).expect("解析 v1 配置");
        let mut config = raw.migrate();
        config.normalize();

        assert_eq!(config.version, CONFIG_VERSION);
        assert_eq!(config.stage_names(), vec!["dev", "test"], "顺序应保持 dev -> test");
        // 关键：凭据键（name）必须还是 dev / test。
        assert_eq!(config.stages[0].name, "dev");
        assert_eq!(config.stages[1].name, "test");
        assert_eq!(config.stages[0].base_url, "http://dev:8080");
        assert_eq!(config.stages[0].username, "u1");
        assert_eq!(config.stages[0].poll_interval_secs, 3);
        assert_eq!(config.stages[1].base_url, "http://test:8080");
        assert!(config.stages[1].insecure_tls);
        assert_eq!(config.stages[1].timeout_minutes, 45);
        assert_eq!(config.services.len(), 1);
        assert_eq!(config.last_selection, vec!["svc-a"]);
    }

    /// v2 正常读取时不该被迁移逻辑改写。
    #[test]
    fn reads_v2_stages_as_is() {
        let v2 = r####"{
          "version": 2,
          "stages": [
            { "name": "dev", "baseUrl": "http://d", "username": "u" },
            { "name": "sit", "baseUrl": "http://s", "username": "u", "abortOnFailure": true },
            { "name": "prod", "baseUrl": "http://p", "username": "u" }
          ],
          "services": [],
          "template": {
            "title": "t", "body": "b", "useFailureTemplate": false,
            "failureTitle": "ft", "failureBody": "fb"
          },
          "lastSelection": []
        }"####;
        let raw: RawConfig = serde_json::from_str(v2).expect("解析 v2 配置");
        let mut config = raw.migrate();
        config.normalize();
        assert_eq!(config.stage_names(), vec!["dev", "sit", "prod"]);
        assert!(config.stages[1].abort_on_failure);
        assert!(!config.stages[0].abort_on_failure);
    }

    #[test]
    fn drops_duplicate_and_empty_stage_names() {
        let mut config = AppConfig {
            stages: vec![
                StageConfig::new("dev"),
                StageConfig::new("dev"),
                StageConfig::new("  "),
                StageConfig::new("prod"),
            ],
            ..Default::default()
        };
        config.normalize();
        assert_eq!(config.stage_names(), vec!["dev", "prod"]);
    }

    #[test]
    fn fills_missing_stage_timings() {
        let mut stage = StageConfig::new("dev");
        stage.poll_interval_secs = 0;
        stage.timeout_minutes = 0;
        let mut config = AppConfig {
            stages: vec![stage],
            ..Default::default()
        };
        config.normalize();
        assert_eq!(config.stages[0].poll_interval_secs, 5);
        assert_eq!(config.stages[0].timeout_minutes, 90);
    }

    /// 真正走一遍「从磁盘读旧配置」的路径，确保老用户升级后
    /// 仍能读到 dev/test 两个环境，且 token 键名不变。
    #[test]
    fn loads_and_migrates_legacy_file_from_disk() {
        let dir = std::env::temp_dir().join(format!(
            "jbt-migrate-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("建临时目录");

        // 完整照抄旧版落盘的结构（含 id / displayName 这些已废弃字段）。
        let legacy = r####"{
          "version": 1,
          "dev": {
            "id": "dev", "displayName": "开发环境 (dev)", "baseUrl": "http://dev:8080/",
            "username": "u1", "insecureTls": false,
            "pollIntervalSecs": 5, "timeoutMinutes": 90
          },
          "test": {
            "id": "test", "displayName": "测试环境 (test)", "baseUrl": "http://test:8080/",
            "username": "u2", "insecureTls": true,
            "pollIntervalSecs": 5, "timeoutMinutes": 90
          },
          "services": [
            { "name": "fsk-esale", "favorite": true, "params": [
              { "key": "BRANCH", "value": "main" }
            ] }
          ],
          "template": {
            "title": "✅ 发布全部完成",
            "body": "{{serviceCount}} 个服务已在 dev 与 test 全部发布成功（耗时 {{duration}}）",
            "useFailureTemplate": true,
            "failureTitle": "❌ 发布存在失败",
            "failureBody": "成功 {{successCount}} 个，失败 {{failedCount}} 个：{{failedList}}"
          },
          "lastSelection": ["fsk-esale"]
        }"####;
        std::fs::write(dir.join("config.json"), legacy).expect("写旧配置");

        let store = ConfigStore::load(&dir);
        let config = store.get().expect("读取配置");

        // 迁移成 v2，且环境名保持 dev / test（凭据键不能变）。
        assert_eq!(config.version, CONFIG_VERSION);
        assert_eq!(config.stage_names(), vec!["dev", "test"]);
        assert_eq!(config.stages[0].base_url, "http://dev:8080/");
        assert_eq!(config.stages[0].username, "u1");
        assert!(config.stages[1].insecure_tls);
        // 服务与勾选记录也要带过来。
        assert_eq!(config.services.len(), 1);
        assert_eq!(config.services[0].name, "fsk-esale");
        assert_eq!(config.services[0].params[0].key, "BRANCH");
        assert_eq!(config.last_selection, vec!["fsk-esale"]);

        // 追加一个环境并落盘后，重新读出来要还在。
        store
            .update(|c| c.stages.push(StageConfig::new("prod")))
            .expect("保存");
        let reloaded = ConfigStore::load(&dir).get().expect("重新读取");
        assert_eq!(reloaded.stage_names(), vec!["dev", "test", "prod"]);

        let _ = std::fs::remove_dir_all(&dir);
    }
}

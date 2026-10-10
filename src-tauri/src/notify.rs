//! 完成后的通知：文案模板渲染 + 系统级桌面通知。

use serde::{Deserialize, Serialize};
use tauri_plugin_notification::NotificationExt;

use crate::config::TemplateConfig;

/// 模板渲染时可用的变量。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateVars {
    /// 本次涉及的服务总数。
    pub service_count: usize,
    pub success_count: usize,
    pub failed_count: usize,
    /// 成功服务名，逗号分隔。
    pub success_list: String,
    /// 失败服务名，逗号分隔。
    pub failed_list: String,
    /// 全部服务名，逗号分隔。
    pub service_list: String,
    /// 总耗时，例如 `3分12秒`。
    pub duration: String,
    /// 整体结论：`成功` 或 `失败`。
    pub status: String,
    /// 完成时刻，例如 `2026-01-02 15:04:05`。
    pub finished_at: String,
    /// 流水线环境链路，例如 `dev → test`。
    pub stage_chain: String,
    /// 环境个数。
    pub stage_count: usize,
    /// 本次通知对应的服务名（通知按单个服务发，所以这条最有辨识度）。
    pub service_name: String,
    /// 该服务实际推进到的最后一级环境。
    pub reached_stage: String,
    /// 出错的那一级环境；没出错时为空串。
    pub failed_stage: String,
}

impl TemplateVars {
    /// 查一个变量的值；未知变量返回 `None`。
    pub fn get(&self, key: &str) -> Option<String> {
        let value = match key {
            "serviceCount" => self.service_count.to_string(),
            "successCount" => self.success_count.to_string(),
            "failedCount" => self.failed_count.to_string(),
            "successList" => self.success_list.clone(),
            "failedList" => self.failed_list.clone(),
            "serviceList" => self.service_list.clone(),
            "duration" => self.duration.clone(),
            "status" => self.status.clone(),
            "finishedAt" => self.finished_at.clone(),
            "stageChain" => self.stage_chain.clone(),
            "stageCount" => self.stage_count.to_string(),
            "serviceName" => self.service_name.clone(),
            "reachedStage" => self.reached_stage.clone(),
            "failedStage" => self.failed_stage.clone(),
            _ => return None,
        };
        Some(value)
    }
}

/// 渲染模板：把 `{{变量}}` 替换成实际值。
///
/// 未知变量原样保留，这样用户能一眼看出自己把名字写错了，
/// 而不是被悄悄替换成空白。
pub fn render_template(template: &str, vars: &TemplateVars) -> String {
    let bytes = template.as_bytes();
    let mut out = String::with_capacity(template.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' && i + 1 < bytes.len() && bytes[i + 1] == b'{' {
            if let Some(rel_end) = template[i + 2..].find("}}") {
                let key = template[i + 2..i + 2 + rel_end].trim();
                match vars.get(key) {
                    Some(value) => out.push_str(&value),
                    None => out.push_str(&template[i..i + 2 + rel_end + 2]),
                }
                i = i + 2 + rel_end + 2;
                continue;
            }
        }
        // 按字符推进，避免切坏多字节字符。
        let ch = template[i..].chars().next().unwrap_or(' ');
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// 通知要发的内容。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationContent {
    pub title: String,
    pub body: String,
    /// 是否有服务失败，前端可据此用不同配色。
    pub failed: bool,
    /// 这次任务是被取消的（不是真的失败）。
    #[serde(default)]
    pub cancelled: bool,
}

/// 根据模板与变量组装通知内容。
///
/// 三种结果各有一套文案：成功、失败、被取消。
pub fn build_content(
    template: &TemplateConfig,
    vars: &TemplateVars,
) -> NotificationContent {
    let failed = vars.failed_count > 0;
    let cancelled = vars.status == "已取消";

    let (title_tpl, body_tpl) = if cancelled {
        (&template.cancelled_title, &template.cancelled_body)
    } else if failed && template.use_failure_template {
        (&template.failure_title, &template.failure_body)
    } else {
        (&template.title, &template.body)
    };

    let mut title = render_template(title_tpl, vars);
    let mut body = render_template(body_tpl, vars);
    // 文案为空时给一个兜底，避免弹出空白通知。
    if title.trim().is_empty() {
        title = if cancelled {
            "任务已取消".to_string()
        } else if failed {
            "发布存在失败".to_string()
        } else {
            "发布全部完成".to_string()
        };
    }
    if body.trim().is_empty() {
        body = format!(
            "成功 {} 个，失败 {} 个",
            vars.success_count, vars.failed_count
        );
    }
    NotificationContent {
        title,
        body,
        failed,
        cancelled,
    }
}

/// 把秒数格式化成 `1小时2分3秒` 这种易读形式。
pub fn humanize_duration(total_secs: u64) -> String {
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;
    if hours > 0 {
        format!("{hours}小时{minutes}分{seconds}秒")
    } else if minutes > 0 {
        format!("{minutes}分{seconds}秒")
    } else {
        format!("{seconds}秒")
    }
}

/// 发送系统级桌面通知。
///
/// 通知发不出去（例如用户关掉了通知权限）不应该影响流水线结果，
/// 所以这里返回 `Err` 由调用方决定是否只记一条日志。
pub fn send_system_notification(
    app: &tauri::AppHandle,
    content: &NotificationContent,
) -> Result<(), String> {
    app.notification()
        .builder()
        .title(&content.title)
        .body(&content.body)
        .show()
        .map_err(|e| format!("发送系统通知失败：{e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars() -> TemplateVars {
        TemplateVars {
            service_count: 3,
            success_count: 2,
            failed_count: 1,
            success_list: "a, b".to_string(),
            failed_list: "c".to_string(),
            service_list: "a, b, c".to_string(),
            duration: "3分12秒".to_string(),
            status: "失败".to_string(),
            finished_at: "2026-01-02 15:04:05".to_string(),
            stage_chain: "dev → sit → prod".to_string(),
            stage_count: 3,
            service_name: "order-service".to_string(),
            reached_stage: "sit".to_string(),
            failed_stage: "sit".to_string(),
        }
    }

    /// 单个服务的通知文案要用到服务名与卡在哪一级。
    #[test]
    fn renders_per_service_variables() {
        let text = render_template(
            "{{serviceName}} 在 {{failedStage}} 失败（链路 {{stageChain}}，走到 {{reachedStage}}）",
            &vars(),
        );
        assert_eq!(
            text,
            "order-service 在 sit 失败（链路 dev → sit → prod，走到 sit）"
        );
    }

    /// 环境链路变量要能渲染，多级流水线的默认文案依赖它。
    #[test]
    fn renders_stage_chain() {
        let text = render_template("已在 {{stageChain}} 发布（共 {{stageCount}} 级）", &vars());
        assert_eq!(text, "已在 dev → sit → prod 发布（共 3 级）");
    }

    #[test]
    fn renders_known_placeholders() {
        let text = render_template("成功 {{successCount}} / {{serviceCount}}", &vars());
        assert_eq!(text, "成功 2 / 3");
    }

    #[test]
    fn keeps_unknown_placeholders_visible() {
        let text = render_template("你好 {{nope}}", &vars());
        assert_eq!(text, "你好 {{nope}}");
    }

    #[test]
    fn failure_uses_failure_template() {
        let template = TemplateConfig::default();
        let content = build_content(&template, &vars());
        assert!(content.failed);
        // 默认文案是按「单个服务」设计的。
        assert_eq!(content.title, "❌ order-service 发布失败");
        assert_eq!(
            content.body,
            "卡在 sit：失败（链路 dev → sit → prod，耗时 3分12秒）"
        );
    }

    /// 成功任务的通知变量。
    #[test]
    fn cancelled_task_uses_its_own_wording() {
        let template = TemplateConfig::default();
        let mut v = vars();
        v.status = "已取消".to_string();
        v.failed_count = 1;
        v.failed_stage = "sit".to_string();
        let content = build_content(&template, &v);
        // 取消要走取消模板，不能借用「失败」那套文案。
        assert!(content.cancelled);
        assert_eq!(content.title, "⊘ order-service 已取消");
        assert_eq!(content.body, "任务被手动取消，停在 sit（耗时 3分12秒）");
    }

    #[test]
    fn success_uses_success_template() {
        let template = TemplateConfig::default();
        let mut v = vars();
        v.failed_count = 0;
        v.failed_list = String::new();
        let content = build_content(&template, &v);
        assert!(!content.failed);
        assert_eq!(content.title, "✅ order-service 发布完成");
        assert_eq!(
            content.body,
            "已在 dev → sit → prod 全部发布成功（耗时 3分12秒）"
        );
    }

    #[test]
    fn formats_durations() {
        assert_eq!(humanize_duration(45), "45秒");
        assert_eq!(humanize_duration(192), "3分12秒");
        assert_eq!(humanize_duration(3725), "1小时2分5秒");
    }
}

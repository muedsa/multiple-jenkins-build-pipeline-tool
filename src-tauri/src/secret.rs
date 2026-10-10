//! Jenkins API Token 的存放。
//!
//! token 不写进配置文件，而是交给系统凭据管理器（Windows 上就是「凭据管理器」），
//! 每个流水线环境对应一条凭据：`用户名` = 环境名，`密码` = API Token。
//!
//! 环境名就是凭据的键，所以**给环境改名等于换一把凭据** ——
//! 设置页里改名后会提示需要重新填 token。

use keyring::Entry;

/// 凭据管理器里显示的服务名，也是同一款应用所有条目的归类前缀。
const SERVICE: &str = "multiple-jenkins-build-pipeline-tool";

/// 某个环境的凭据条目是否已经存在。
pub fn has_token(stage: &str) -> bool {
    load_token(stage).is_some()
}

/// 读取 token。没有存过（或凭据管理器不可用）时返回 `None`。
pub fn load_token(stage: &str) -> Option<String> {
    let entry = Entry::new(SERVICE, stage).ok()?;
    match entry.get_password() {
        Ok(token) if !token.trim().is_empty() => Some(token),
        // NoEntry 表示还没存过，属于正常情况；其它错误（例如凭据库不可用）也按“没有”处理，
        // 由调用方给出更友好的提示。
        _ => None,
    }
}

/// 保存 token。传空字符串等同于删除。
pub fn save_token(stage: &str, token: &str) -> Result<(), String> {
    let entry = Entry::new(SERVICE, stage)
        .map_err(|e| format!("无法访问系统凭据管理器：{e}"))?;
    if token.trim().is_empty() {
        // 条目不存在时删除会报错，这里忽略即可。
        let _ = entry.delete_credential();
        return Ok(());
    }
    entry
        .set_password(token)
        .map_err(|e| format!("保存 API Token 到系统凭据管理器失败：{e}"))
}

/// 删除 token。
pub fn clear_token(stage: &str) -> Result<(), String> {
    save_token(stage, "")
}

/// 环境改名后把旧凭据挪到新名字下，避免用户以为 token“丢了”。
///
/// 旧名字没有 token 时什么都不做；新名字已经有 token 时不覆盖。
pub fn rename_token(old_name: &str, new_name: &str) -> Result<(), String> {
    if old_name.trim() == new_name.trim() {
        return Ok(());
    }
    let Some(token) = load_token(old_name) else {
        return Ok(());
    };
    if has_token(new_name) {
        // 新名字已经有凭据了，尊重已有的，不覆盖。
        return Ok(());
    }
    save_token(new_name, &token)?;
    clear_token(old_name)
}

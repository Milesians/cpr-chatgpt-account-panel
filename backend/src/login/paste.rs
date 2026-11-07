//! 快速导入文本解析、去重与 CPR 查重（`邮箱----密码----2FA[----team|personal|个人]`）

use std::collections::{HashMap, HashSet};

use gateway_plugin_sdk::call::host::AuthRuntimeAccount;
use serde::{Deserialize, Serialize};

use crate::chatgpt::{is_team, supported};
use crate::login::totp::validate_totp_secret;

pub const KIND_PERSONAL: &str = "personal";
pub const KIND_TEAM: &str = "team";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PasteItem {
    pub line: usize,
    pub email: String,
    pub kind: String,
    pub has_password: bool,
    pub has_totp: bool,
    /// `"create"`（待新建到 CPR）、`"update"`（已在 CPR，可更新凭据/重登）或 `"skip"`（跳过/有误）
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub existing_account_id: Option<String>,
    pub in_cpr: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// 仅存在于内存中的解析凭据（绝不序列化到响应或日志）
#[derive(Clone)]
pub struct ParsedSecretRow {
    pub item: PasteItem,
    pub password: String,
    pub totp_secret: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PasteReport {
    pub applied: bool,
    pub lines: usize,
    pub created: usize,
    pub updated: usize,
    pub skipped: usize,
    pub existing_in_cpr: usize,
    pub items: Vec<PasteItem>,
}

/// 解析多行粘贴文本并与 CPR 现有账号查重
#[must_use]
pub fn parse_paste_lines(
    raw_text: &str,
    cpr_accounts: &[AuthRuntimeAccount],
) -> (PasteReport, Vec<ParsedSecretRow>) {
    let normalized = raw_text.replace("\r\n", "\n").replace('\r', "\n");
    let mut existing_by_key: HashMap<(String, String), Vec<String>> = HashMap::new();
    for acct in cpr_accounts {
        if !supported(acct) {
            continue;
        }
        let Some(email) = acct
            .email
            .as_deref()
            .map(|e| e.trim().to_ascii_lowercase())
            .filter(|e| !e.is_empty())
        else {
            continue;
        };
        let kind = account_kind(acct.plan_type.as_deref()).to_owned();
        existing_by_key
            .entry((email, kind))
            .or_default()
            .push(acct.account_id.clone());
    }

    let mut report = PasteReport::default();
    let mut rows = Vec::new();
    let mut seen_keys = HashSet::new();

    for (idx, raw_line) in normalized.split('\n').enumerate() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        report.lines += 1;
        let line_no = idx + 1;
        match parse_single_line(line_no, trimmed) {
            Err(skip_item) => {
                report.skipped += 1;
                report.items.push(*skip_item);
            }
            Ok((mut item, password, totp_secret)) => {
                let key = (item.email.clone(), item.kind.clone());
                if !seen_keys.insert(key.clone()) {
                    item.action = "skip".to_owned();
                    item.code = Some("duplicate_in_input".to_owned());
                    item.message = Some("粘贴内容中存在重复的邮箱与类型组合".to_owned());
                    report.skipped += 1;
                    report.items.push(item);
                    continue;
                }
                if let Some(matches) = existing_by_key.get(&key) {
                    if matches.len() > 1 {
                        item.action = "skip".to_owned();
                        item.in_cpr = true;
                        item.code = Some("ambiguous_existing".to_owned());
                        item.message = Some("CPR 中存在多个同邮箱且同类型的账号".to_owned());
                        report.skipped += 1;
                        report.items.push(item);
                        continue;
                    }
                    item.in_cpr = true;
                    item.existing_account_id = Some(matches[0].clone());
                    item.action = "update".to_owned();
                    item.code = Some("already_in_source".to_owned());
                    item.message = Some("该账号已存在于 CPR，可更新保存凭据或重新登录".to_owned());
                    report.updated += 1;
                    report.existing_in_cpr += 1;
                } else {
                    item.action = "create".to_owned();
                    report.created += 1;
                }
                report.items.push(item.clone());
                rows.push(ParsedSecretRow {
                    item,
                    password,
                    totp_secret,
                });
            }
        }
    }

    (report, rows)
}

/// 根据 CPR 套餐判断账号类型（不知道就算 `personal`）
#[must_use]
pub fn account_kind(plan: Option<&str>) -> &'static str {
    if is_team(plan) == Some(true) {
        KIND_TEAM
    } else {
        KIND_PERSONAL
    }
}

/// 解析用户输入的账号类型标识
pub fn normalize_kind(raw: &str) -> Result<&'static str, &'static str> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "" | "personal" | "个人" => Ok(KIND_PERSONAL),
        "team" | "团队" => Ok(KIND_TEAM),
        _ => Err("invalid_kind"),
    }
}

/// 校验邮箱格式
#[must_use]
pub fn is_valid_email(email: &str) -> bool {
    let s = email.trim();
    if s.is_empty() || s.len() > 254 || s.bytes().any(|b| b.is_ascii_whitespace()) {
        return false;
    }
    let Some((local, domain)) = s.split_once('@') else {
        return false;
    };
    if local.is_empty() || domain.is_empty() || domain.contains('@') {
        return false;
    }
    let parts: Vec<&str> = domain.split('.').collect();
    if parts.len() < 2 || parts.iter().any(|p| p.is_empty()) {
        return false;
    }
    true
}

fn parse_single_line(
    line_no: usize,
    line: &str,
) -> Result<(PasteItem, String, String), Box<PasteItem>> {
    let parts: Vec<&str> = line.split("----").collect();
    if !(2..=4).contains(&parts.len()) {
        return Err(Box::new(skip_row(
            line_no,
            "",
            KIND_PERSONAL,
            "invalid_line_format",
            "格式应为：邮箱----密码----2FA（行尾可加 ----team 或 ----personal）",
        )));
    }
    let email = parts[0].trim().to_ascii_lowercase();
    if email.is_empty() {
        return Err(Box::new(skip_row(
            line_no,
            "",
            KIND_PERSONAL,
            "missing_email",
            "缺少邮箱地址",
        )));
    }
    if !is_valid_email(&email) {
        return Err(Box::new(skip_row(
            line_no,
            &email,
            KIND_PERSONAL,
            "invalid_email",
            "邮箱地址格式无效",
        )));
    }
    let password = parts[1].trim().to_owned();
    if password.is_empty() {
        return Err(Box::new(skip_row(
            line_no,
            &email,
            KIND_PERSONAL,
            "missing_password",
            "缺少登录密码",
        )));
    }
    let mut totp_secret = String::new();
    if parts.len() >= 3 && !parts[2].trim().is_empty() {
        match validate_totp_secret(parts[2]) {
            Ok(normalized) => totp_secret = normalized,
            Err(_) => {
                return Err(Box::new(skip_row(
                    line_no,
                    &email,
                    KIND_PERSONAL,
                    "invalid_totp",
                    "2FA 密钥无效（需为 Base32 密钥或 6位/30秒/SHA1 的 otpauth://totp/ 链接）",
                )));
            }
        }
    }
    let kind = if parts.len() == 4 {
        match normalize_kind(parts[3]) {
            Ok(k) => k,
            Err(_) => {
                return Err(Box::new(skip_row(
                    line_no,
                    &email,
                    KIND_PERSONAL,
                    "invalid_kind",
                    "账号类型必须是 team、personal 或 个人",
                )));
            }
        }
    } else {
        KIND_PERSONAL
    };

    Ok((
        PasteItem {
            line: line_no,
            email,
            kind: kind.to_owned(),
            has_password: true,
            has_totp: !totp_secret.is_empty(),
            action: "create".to_owned(),
            existing_account_id: None,
            in_cpr: false,
            code: None,
            message: None,
        },
        password,
        totp_secret,
    ))
}

fn skip_row(line: usize, email: &str, kind: &str, code: &str, message: &str) -> PasteItem {
    PasteItem {
        line,
        email: email.to_owned(),
        kind: kind.to_owned(),
        has_password: false,
        has_totp: false,
        action: "skip".to_owned(),
        existing_account_id: None,
        in_cpr: false,
        code: Some(code.to_owned()),
        message: Some(message.to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_cpr_account(id: &str, email: &str, plan: &str) -> AuthRuntimeAccount {
        AuthRuntimeAccount {
            account_id: id.to_owned(),
            provider_id: "openai".to_owned(),
            credential_revision: 1,
            name: id.to_owned(),
            email: Some(email.to_owned()),
            upstream_user_id: None,
            upstream_account_id: Some(format!("ws-{id}")),
            plan_type: Some(plan.to_owned()),
            authentication_kind: "oauth".to_owned(),
            enabled: true,
            credential_state: "ready".to_owned(),
            has_refresh_token: true,
            access_token_expires_at_ms: None,
            next_refresh_at_ms: None,
        }
    }

    #[test]
    fn parses_and_deduplicates_paste_lines() {
        let existing = vec![sample_cpr_account("acct_1", "existing@example.com", "team")];
        let text = "\
            # comment\n\
            new1@example.com----pass1----JBSWY3DPEHPK3PXP\n\
            new2@example.com----pass2--------team\n\
            new3@example.com----pass3----otpauth://totp/OpenAI:new3@example.com?secret=JBSWY3DPEHPK3PXP----个人\n\
            existing@example.com----pass4--------team\n\
            new1@example.com----pass5----JBSWY3DPEHPK3PXP\n\
            bad-email----pass6\n\
        ";
        let (report, rows) = parse_paste_lines(text, &existing);
        assert_eq!(report.lines, 6);
        assert_eq!(report.created, 3);
        assert_eq!(report.updated, 1);
        assert_eq!(report.skipped, 2);
        assert_eq!(rows.len(), 4);
        assert_eq!(report.items[0].action, "create");
        assert_eq!(report.items[0].kind, "personal");
        assert!(report.items[0].has_totp);
        assert_eq!(report.items[1].action, "create");
        assert_eq!(report.items[1].kind, "team");
        assert!(!report.items[1].has_totp);
        assert_eq!(report.items[2].kind, "personal");
        assert!(report.items[2].has_totp);
        assert_eq!(report.items[3].action, "update");
        assert_eq!(
            report.items[3].existing_account_id.as_deref(),
            Some("acct_1")
        );
        assert_eq!(report.items[4].code.as_deref(), Some("duplicate_in_input"));
        assert_eq!(report.items[5].code.as_deref(), Some("invalid_email"));
    }
}

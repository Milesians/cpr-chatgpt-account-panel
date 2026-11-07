//! 登录设备：ChatGPT 设置页列出的这个登录的设备（`GET /backend-api/accounts/sessions`），
//! 以及登出（`POST /backend-api/accounts/sessions/revoke`）。都是 ChatGPT 的内部接口，
//! 回答不像设置页的就拒绝，不猜
//!
//! 会话 ID 不出这个模块：页面只拿它的哈希；登出前重新列一次、按哈希找，所以只能登出现在
//! 列着的设备。CPR 自己令牌所在的会话（`is_current_device`，或令牌 claims 里的
//! `session_id`）不能单独登出，否则 CPR 的令牌就失效了；“登出所有设备”会连它一起登出，
//! 放在最后，之后要重新登录写回 CPR

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::chatgpt::{Problem, Session};
use crate::clock::now_ms;

const MAX_DEVICES: usize = 500;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Device {
    /// 登出时用来指认设备；不是会话 ID
    pub id: String,
    pub name: String,
    pub platform: String,
    /// 在这台设备上登录的客户端（ChatGPT、Codex…）
    pub apps: Vec<String>,
    /// ChatGPT 说令牌的会话在这台设备上
    pub current: bool,
    /// CPR 令牌所在的会话：不能单独登出
    pub in_use: bool,
    /// 受信任的设备免二次验证
    pub trusted: bool,
    /// 受信任但已经没有会话的设备，没什么可登出的
    pub signed_out: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_signed_in_ms: Option<i64>,
    pub city: String,
    pub region: String,
    pub country: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Devices {
    pub devices: Vec<Device>,
    /// 恰好一台是 CPR 的会话：只有这时才能一起登出其他设备
    pub in_use_known: bool,
    pub checked_at_ms: i64,
}

/// 一台设备和它的会话 ID（会话 ID 只留在这里）
#[derive(Debug, Clone)]
pub struct Listed {
    pub device: Device,
    session: String,
}

#[derive(Deserialize)]
struct App {
    #[serde(default)]
    client_type: String,
    #[serde(default)]
    client_name: String,
}

#[derive(Deserialize)]
struct Row {
    session_id: Option<String>,
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    device_model: String,
    #[serde(default)]
    platform: String,
    is_current_device: Option<bool>,
    #[serde(default)]
    is_trusted_device: bool,
    #[serde(default)]
    last_signed_in_timestamp_second: f64,
    #[serde(default)]
    last_signed_in_city: String,
    #[serde(default)]
    last_signed_in_region_code: String,
    #[serde(default)]
    last_signed_in_country: String,
    #[serde(default)]
    app_sessions: Vec<App>,
}

/// 短而可打印的显示文字，别的变成空
fn label(s: &str, fallback: &str) -> String {
    let s = if s.trim().is_empty() { fallback } else { s }.trim();
    if s.len() > 120 || s.chars().any(char::is_control) {
        String::new()
    } else {
        s.to_owned()
    }
}

fn valid_session(sid: &str) -> bool {
    sid.len() <= 512 && sid.bytes().all(|b| b > b' ' && b != 0x7f)
}

fn hash(prefix: &str, s: &str) -> String {
    let sum = Sha256::digest(format!("{prefix}{s}").as_bytes());
    hex::encode(&sum[..12])
}

fn invalid() -> Problem {
    Problem::new("invalid_response", "ChatGPT 返回了无法识别的设备列表")
}

/// 设备列表的回答；`token_session` 是令牌 claims 里的会话 ID（可能为空）
///
/// # Errors
/// 回答不像设置页的（有分页、缺字段、会话 ID 重复或不合法、太多）时返回错误
pub fn parse(body: &[u8], token_session: &str) -> Result<Vec<Listed>, Problem> {
    let v: BTreeMap<String, Value> = serde_json::from_slice(body).map_err(|_| invalid())?;
    // 设置页只读一页；有分页的迹象说明这一页不全
    for k in ["has_more", "next_cursor", "cursor", "pagination"] {
        if v.get(k).is_some_and(|x| {
            !matches!(x, Value::Null | Value::Bool(false)) && x != &json!("") && x != &json!(0)
        }) {
            return Err(invalid());
        }
    }
    let rows: Vec<Row> = v
        .get("devices")
        .cloned()
        .and_then(|d| serde_json::from_value(d).ok())
        .ok_or_else(invalid)?;
    if rows.len() > MAX_DEVICES {
        return Err(invalid());
    }
    let mut seen = BTreeSet::new();
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let current = r.is_current_device.ok_or_else(invalid)?;
        let sid = r
            .session_id
            .as_deref()
            .unwrap_or_default()
            .trim()
            .to_owned();
        if sid.is_empty() && (current || !r.is_trusted_device) {
            return Err(invalid());
        }
        if !sid.is_empty() && (!valid_session(&sid) || !seen.insert(sid.clone())) {
            return Err(invalid());
        }
        let name = label(&r.display_name, &r.device_model);
        let platform = label(&r.platform, "");
        let mut apps: Vec<String> = Vec::new();
        for a in &r.app_sessions {
            let n = label(&a.client_name, "");
            if a.client_type == "site"
                || n.is_empty()
                || apps.iter().any(|x| x.eq_ignore_ascii_case(&n))
            {
                continue;
            }
            apps.push(n);
        }
        let ts = r.last_signed_in_timestamp_second;
        let id = if sid.is_empty() {
            format!("x{}", hash("signed-out:", &format!("{name}|{platform}")))
        } else {
            hash("device:", &sid)
        };
        out.push(Listed {
            device: Device {
                id,
                in_use: !sid.is_empty()
                    && (current || (!token_session.is_empty() && sid == token_session)),
                name,
                platform,
                apps,
                current,
                trusted: r.is_trusted_device,
                signed_out: sid.is_empty(),
                #[allow(clippy::cast_possible_truncation)]
                last_signed_in_ms: (ts > 0.0 && ts < 1e11).then_some((ts * 1000.0) as i64),
                city: label(&r.last_signed_in_city, ""),
                region: label(&r.last_signed_in_region_code, ""),
                country: label(&r.last_signed_in_country, ""),
            },
            session: sid,
        });
    }
    Ok(out)
}

async fn list(session: &Session<'_>) -> Result<Vec<Listed>, Problem> {
    let body = session.get("/backend-api/accounts/sessions").await?;
    parse(&body, &session.token_session())
}

async fn revoke(session: &Session<'_>, sid: &str) -> Result<(), Problem> {
    let body = session
        .post_json(
            "/backend-api/accounts/sessions/revoke",
            &json!({"session_id": sid}),
        )
        .await?;
    // 回答为空或是 JSON 都算确认；别的说明接口变了
    if !body.iter().all(u8::is_ascii_whitespace) && serde_json::from_slice::<Value>(&body).is_err()
    {
        return Err(unconfirmed(
            "ChatGPT 对登出请求的回答无法识别，请刷新设备列表核对",
        ));
    }
    Ok(())
}

fn unconfirmed(message: &str) -> Problem {
    Problem::new("revoke_unconfirmed", message)
}

/// 列出设备
///
/// # Errors
/// 账号不能查、上游拒绝或回答无法识别时返回错误
pub async fn devices(session: &Session<'_>) -> Result<Devices, Problem> {
    let all = list(session).await?;
    let in_use = all.iter().filter(|d| d.device.in_use).count();
    Ok(Devices {
        devices: all.into_iter().map(|d| d.device).collect(),
        in_use_known: in_use == 1,
        checked_at_ms: now_ms(),
    })
}

/// 登出一台设备（按页面拿到的 ID）
///
/// # Errors
/// 设备不在列表里、是 CPR 的会话、已经登出、上游拒绝或登出后仍列着时返回错误
pub async fn sign_out(session: &Session<'_>, device: &str) -> Result<(), Problem> {
    let all = list(session).await?;
    let Some(target) = all.iter().find(|d| d.device.id == device) else {
        return Err(Problem::new(
            "device_not_found",
            "设备列表已变化，请刷新后重试",
        ));
    };
    if target.device.in_use {
        return Err(Problem::new(
            "device_in_use",
            "这是 CPR 正在使用的登录，不能单独登出",
        ));
    }
    if target.session.is_empty() {
        return Err(Problem::new("device_signed_out", "这台设备已经登出"));
    }
    revoke(session, &target.session).await?;
    let after = list(session)
        .await
        .map_err(|_| unconfirmed("已发出登出请求，但未能确认结果，请刷新设备列表核对"))?;
    if after.iter().any(|d| d.session == target.session) {
        return Err(unconfirmed("ChatGPT 仍列出这台设备，请刷新设备列表核对"));
    }
    Ok(())
}

/// 登出 CPR 会话以外的全部设备；CPR 的会话必须能唯一确定。返回登出了几台
///
/// # Errors
/// 认不出 CPR 的会话、中途列表变化（出现新设备或 CPR 的会话不在了）、上游拒绝或登出后
/// 仍有别的设备时返回错误（附已登出的台数）
pub async fn sign_out_others(session: &Session<'_>) -> Result<usize, (usize, Problem)> {
    let unknown = || {
        Problem::new(
            "in_use_unknown",
            "无法确定 CPR 正在使用哪个登录，请逐台登出",
        )
    };
    let all = list(session).await.map_err(|p| (0, p))?;
    let mut keep = None;
    let mut others = BTreeSet::new();
    for d in &all {
        if d.device.in_use {
            if keep.is_some() {
                return Err((0, unknown()));
            }
            keep = Some(d.session.clone());
        } else if !d.session.is_empty() {
            others.insert(d.session.clone());
        }
    }
    let keep = keep.ok_or_else(|| (0, unknown()))?;
    let mut done = 0;
    for sid in &others {
        // 中途新登录的设备不擅自登出，CPR 的会话也必须还在
        let now = list(session)
            .await
            .map_err(|_| (done, unconfirmed("登出中途无法读取设备列表，请刷新核对")))?;
        if changed(&now, &keep, &others) {
            return Err((
                done,
                Problem::new(
                    "devices_changed",
                    "设备列表在登出过程中变化，已停止，请刷新核对",
                ),
            ));
        }
        if !now.iter().any(|d| &d.session == sid) {
            continue;
        }
        revoke(session, sid).await.map_err(|p| (done, p))?;
        done += 1;
    }
    let after = list(session).await.map_err(|_| {
        (
            done,
            unconfirmed("已发出登出请求，但未能确认结果，请刷新设备列表核对"),
        )
    })?;
    if after
        .iter()
        .any(|d| !d.session.is_empty() && d.session != keep)
    {
        return Err((
            done,
            unconfirmed("ChatGPT 仍列出其他设备，请刷新设备列表核对"),
        ));
    }
    Ok(done)
}

fn changed(now: &[Listed], keep: &str, allowed: &BTreeSet<String>) -> bool {
    let mut kept = false;
    for d in now {
        if d.session.is_empty() {
            continue;
        }
        if d.session == keep {
            kept = d.device.in_use;
        } else if !allowed.contains(&d.session) {
            return true;
        }
    }
    !kept
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct AllSignedOut {
    pub signed_out: usize,
    /// CPR 自己的会话也登出了：它的令牌不能用了，要重新登录写回 CPR
    pub cpr_voided: bool,
}

/// 登出全部设备，连 CPR 的会话一起：先登出别的，CPR 的放最后（请求用的就是它的令牌）。
/// 期间新登录的设备也登出；登出后还列着就停
///
/// # Errors
/// 上游拒绝、令牌中途失效（CPR 的会话没认出来、跟着别的设备一起登出了）或设备一直
/// 冒出来时返回错误（附已完成的部分）
pub async fn sign_out_all(session: &Session<'_>) -> Result<AllSignedOut, (AllSignedOut, Problem)> {
    let mut out = AllSignedOut::default();
    let mut revoked = BTreeSet::new();
    for _ in 0..=2 * MAX_DEVICES {
        let now = match list(session).await {
            Ok(now) => now,
            // 令牌随它的会话一起没了，什么也读不到了
            Err(_) if out.cpr_voided => return Ok(out),
            Err(p) if out.signed_out == 0 => return Err((out, p)),
            Err(p) if matches!(p.code, "token_revoked" | "oauth_unauthorized") => {
                out.cpr_voided = true;
                return Err((
                    out,
                    Problem::new(
                        "cpr_voided_early",
                        "CPR 的令牌已随某台设备登出而失效，可能还有设备没登出；请重新登录写回 CPR 后再查",
                    ),
                ));
            }
            Err(_) => {
                return Err((out, unconfirmed("登出中途无法读取设备列表，请刷新核对")));
            }
        };
        // 先挑不是 CPR 会话的
        let next = now
            .iter()
            .filter(|d| !d.session.is_empty())
            .min_by_key(|d| d.device.in_use);
        let Some(next) = next else {
            return Ok(out);
        };
        if !revoked.insert(next.session.clone()) {
            return Err((
                out,
                unconfirmed("ChatGPT 仍列出已登出的设备，请刷新设备列表核对"),
            ));
        }
        revoke(session, &next.session)
            .await
            .map_err(|p| (out.clone(), p))?;
        out.signed_out += 1;
        if next.device.in_use {
            out.cpr_voided = true;
        }
    }
    Err((
        out,
        Problem::new("devices_changed", "设备不断出现新登录，已停止，请刷新核对"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(rows: &Value) -> Vec<u8> {
        serde_json::to_vec(&json!({"devices": rows})).unwrap()
    }

    #[test]
    fn parses_devices_and_hides_sessions() {
        let b = body(&json!([
            {"session_id": "s1", "display_name": "MacBook", "platform": "macOS", "is_current_device": true,
             "is_trusted_device": true, "last_signed_in_timestamp_second": 1_800_000_000.5,
             "last_signed_in_city": "Tokyo", "app_sessions": [{"client_type": "app", "client_name": "Codex"},
             {"client_type": "app", "client_name": "codex"}, {"client_type": "site", "client_name": "Web"}]},
            {"session_id": "s2", "device_model": "iPhone", "is_current_device": false},
            {"session_id": null, "display_name": "Old", "is_current_device": false, "is_trusted_device": true},
        ]));
        let all = parse(&b, "").unwrap();
        assert_eq!(all.len(), 3);
        assert!(all[0].device.in_use && all[0].device.current);
        assert_eq!(all[0].device.apps, vec!["Codex"]);
        assert_eq!(all[0].device.last_signed_in_ms, Some(1_800_000_000_500));
        assert_eq!(all[1].device.name, "iPhone");
        assert!(!all[1].device.in_use);
        assert!(all[2].device.signed_out && all[2].device.id.starts_with('x'));
        let text =
            serde_json::to_string(&all.iter().map(|d| &d.device).collect::<Vec<_>>()).unwrap();
        assert!(!text.contains("\"s1\"") && !text.contains("\"s2\""));
        assert_eq!(all[0].device.id, hash("device:", "s1"));
    }

    #[test]
    fn token_claim_marks_cprs_session() {
        let b = body(&json!([
            {"session_id": "s1", "is_current_device": false},
            {"session_id": "s2", "is_current_device": false},
        ]));
        let all = parse(&b, "s2").unwrap();
        assert!(!all[0].device.in_use && all[1].device.in_use);
    }

    #[test]
    fn refuses_answers_unlike_the_page() {
        assert!(parse(br#"{"devices": [], "has_more": true}"#, "").is_err());
        assert!(parse(br#"{"devices": [], "has_more": false, "cursor": null}"#, "").is_ok());
        assert!(parse(br#"{}"#, "").is_err());
        assert!(
            parse(&body(&json!([{"session_id": "s1"}])), "").is_err(),
            "is_current_device is required"
        );
        assert!(
            parse(
                &body(&json!([{"session_id": null, "is_current_device": false}])),
                ""
            )
            .is_err()
        );
        assert!(
            parse(&body(&json!([{"session_id": "s", "is_current_device": false}, {"session_id": "s", "is_current_device": false}])), "")
                .is_err()
        );
        assert!(
            parse(
                &body(&json!([{"session_id": "a b", "is_current_device": false}])),
                ""
            )
            .is_err()
        );
    }

    #[test]
    fn labels_drop_odd_text() {
        assert_eq!(label("  Mac ", ""), "Mac");
        assert_eq!(label("", "iPhone"), "iPhone");
        assert_eq!(label("a\nb", ""), "");
        assert_eq!(label(&"x".repeat(121), ""), "");
    }
}

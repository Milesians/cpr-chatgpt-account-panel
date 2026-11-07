//! 以单个 CPR 账号的身份读 ChatGPT 的内部接口
//!
//! 每次请求前现读账号的凭据（只放内存），经 `host.http.account` 走账号在 CPR 绑定的
//! 代理（没绑就直连；绑了但不可用就失败，不回退直连），只发 GET、不跟随跳转、
//! 不刷新令牌。User-Agent 和 Originator 取 CPR 当前的 Codex 客户端身份：宿主的 TLS
//! 栈是 rustls，和 Codex CLI 一致，不冒充浏览器

use gateway_plugin_sdk::{
    ErrorCode, PluginFault,
    call::host::{AccountHttpRequest, AuthRuntimeAccount, HttpRequest, LogLevel},
    client::HostClient,
};
use serde::Serialize;
use serde_json::{Value, json};

use crate::clock::now_ms;
use crate::host;

pub const BASE: &str = "https://chatgpt.com";
const MAX_BODY: usize = 1 << 20;
const MAX_REFUSAL_BODY: usize = 8 << 10;

/// 浏览器能看的错误：固定的代码和中文说明，不带上游正文或连接细节
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Problem {
    pub code: &'static str,
    pub message: String,
}

impl Problem {
    #[must_use]
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// 令牌被拒、挑战页、限流：整次查询停下，不再发后面的请求
    #[must_use]
    pub fn stops_query(&self) -> bool {
        matches!(
            self.code,
            "token_revoked"
                | "oauth_unauthorized"
                | "upstream_challenge"
                | "rate_limited"
                | "credential_changed"
                | "account_unavailable"
        )
    }
}

/// 只认 CPR 的 OpenAI OAuth 账号
#[must_use]
pub fn supported(account: &AuthRuntimeAccount) -> bool {
    account.provider_id == "openai" && account.authentication_kind == "oauth"
}

/// 个人号还是 Team（team/business/enterprise/edu/free_workspace）；套餐不知道是 None
#[must_use]
pub fn is_team(plan: Option<&str>) -> Option<bool> {
    let p = plan?.trim().to_ascii_lowercase();
    if p.is_empty() {
        return None;
    }
    Some(matches!(
        p.as_str(),
        "team" | "business" | "enterprise" | "edu" | "free_workspace"
    ))
}

/// 发请求前就能看出来的账号问题
#[must_use]
pub fn blocker(account: &AuthRuntimeAccount, now: i64) -> Option<Problem> {
    if !supported(account) {
        return Some(Problem::new(
            "unsupported_account",
            "只支持 OpenAI OAuth 账号",
        ));
    }
    if !account.enabled {
        return Some(Problem::new(
            "account_unavailable",
            "账号在 CPR 里已停用，停用的账号不能经它的代理发请求",
        ));
    }
    match account.credential_state.as_str() {
        "expired" => {
            return Some(Problem::new(
                "credential_state",
                "CPR 标记凭据已失效（expired），需要重新登录",
            ));
        }
        "banned" => {
            return Some(Problem::new(
                "credential_state",
                "CPR 标记账号被封禁（banned）",
            ));
        }
        "invalid" => {
            return Some(Problem::new(
                "credential_state",
                "CPR 标记凭据无效（invalid），需要重新登录",
            ));
        }
        _ => {}
    }
    if account
        .access_token_expires_at_ms
        .is_some_and(|exp| exp <= now + 30_000)
    {
        return Some(Problem::new(
            "token_expired",
            "访问令牌已过期或即将过期。插件不刷新令牌（刷新令牌会轮换），等 CPR 自动刷新后再试",
        ));
    }
    if account
        .upstream_account_id
        .as_deref()
        .unwrap_or_default()
        .is_empty()
    {
        return Some(Problem::new("no_credentials", "账号缺少 ChatGPT 账号 ID"));
    }
    None
}

/// 一个账号这次查询用的凭据与身份。`token` 是密钥：不能进日志、错误或响应
pub struct Session<'a> {
    host: &'a HostClient,
    pub account: AuthRuntimeAccount,
    revision: u64,
    token: String,
    /// ChatGPT 的 workspace（`Chatgpt-Account-Id`），Team 号不带就被拒
    pub workspace: String,
    user_agent: String,
    originator: String,
}

impl<'a> Session<'a> {
    /// 读账号、检查能不能查、现读凭据和 CPR 的客户端身份
    ///
    /// # Errors
    /// 账号不存在、不支持、凭据不可用或宿主回调失败时返回错误
    pub async fn open(host: &'a HostClient, account_id: &str) -> Result<Self, Problem> {
        let account = host::account(host, account_id)
            .await
            .map_err(|e| host_problem(&e))?;
        if let Some(p) = blocker(&account, now_ms()) {
            return Err(p);
        }
        let credential = host::credential(host, account_id)
            .await
            .map_err(|e| host_problem(&e))?;
        let token = credential
            .facts
            .material
            .get("access_token")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_owned();
        let workspace = credential
            .facts
            .upstream_account_id
            .clone()
            .or_else(|| account.upstream_account_id.clone())
            .unwrap_or_default();
        if !header_safe(&token) || !header_safe(&workspace) {
            return Err(Problem::new(
                "no_credentials",
                "账号缺少可用的访问令牌或 ChatGPT 账号 ID",
            ));
        }
        let profile = host::client_profile(host)
            .await
            .map_err(|_| Problem::new("host_callback", "读取 CPR 的 Codex 客户端身份失败"))?;
        let field = |name: &str| {
            profile
                .get(name)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        let (user_agent, originator) = (field("userAgent"), field("originator"));
        if !header_safe(&user_agent) || !header_safe(&originator) {
            return Err(Problem::new(
                "host_callback",
                "CPR 的 Codex 客户端身份不能用于请求头",
            ));
        }
        Ok(Self {
            host,
            revision: credential.credential_revision,
            account,
            token,
            workspace,
            user_agent,
            originator,
        })
    }

    /// 宿主给的套餐（可能比 ChatGPT 的旧）
    #[must_use]
    pub fn plan(&self) -> Option<&str> {
        self.account.plan_type.as_deref()
    }

    /// 把一段要给浏览器的文字里的凭据抹掉
    #[must_use]
    pub fn leaks(&self, s: &str) -> bool {
        s.contains(&self.token)
    }

    /// GET 一个 ChatGPT 地址（`path_and_query` 以 `/backend-api/` 开头），返回 200 的正文
    ///
    /// # Errors
    /// 宿主、网络或上游拒绝时返回浏览器安全的错误
    pub async fn get(&self, path_and_query: &str) -> Result<Vec<u8>, Problem> {
        if !path_and_query.starts_with("/backend-api/") || !header_safe(path_and_query) {
            return Err(Problem::new("invalid_request", "请求地址无效"));
        }
        let headers = vec![
            (
                "Authorization".to_owned(),
                format!("Bearer {}", self.token).into_bytes(),
            ),
            ("Accept".to_owned(), b"application/json".to_vec()),
            ("User-Agent".to_owned(), self.user_agent.as_bytes().to_vec()),
            ("Originator".to_owned(), self.originator.as_bytes().to_vec()),
            (
                "Chatgpt-Account-Id".to_owned(),
                self.workspace.as_bytes().to_vec(),
            ),
        ];
        let response = self
            .host
            .account_http(
                AccountHttpRequest {
                    account_id: self.account.account_id.clone(),
                    credential_revision: self.revision,
                    request: HttpRequest {
                        method: "GET".into(),
                        url: format!("{BASE}{path_and_query}"),
                        headers,
                    },
                },
                Vec::new(),
            )
            .await
            .map_err(|e| host_problem(&e))?;
        let status = response.status;
        let challenge = response.headers.iter().any(|(k, v)| {
            k.eq_ignore_ascii_case("cf-mitigated") && v.eq_ignore_ascii_case(b"challenge")
        });
        let limit = if status == 200 {
            MAX_BODY
        } else {
            MAX_REFUSAL_BODY
        };
        let body = response.body.collect(limit + 1).await;
        if status == 200 {
            return body.map_err(|e| {
                if e.code == ErrorCode::Capacity {
                    Problem::new("invalid_response", "ChatGPT 返回的数据太大")
                } else {
                    host_problem(&e)
                }
            });
        }
        let reason = body.map(|b| refusal_reason(&b)).unwrap_or_default();
        let problem = refusal(status, challenge, &reason);
        let mut reason = reason;
        if self.leaks(&reason) {
            reason.clear();
        }
        host::log(
            self.host,
            "chatgpt_refused",
            LogLevel::Warn,
            json!({"status": status, "code": problem.code, "reason": reason}),
        )
        .await;
        Err(problem)
    }
}

/// 非 200 的回答归类
#[must_use]
pub fn refusal(status: u16, challenge: bool, reason: &str) -> Problem {
    match status {
        401 if reason.starts_with("token_revoked") => Problem::new(
            "token_revoked",
            "这个账号的令牌已被 ChatGPT 吊销（登录被登出或改过密码），需要重新登录",
        ),
        401 => Problem::new(
            "oauth_unauthorized",
            "ChatGPT 拒绝了当前 OAuth 凭据，请检查 CPR 里的账号授权",
        ),
        403 if challenge => Problem::new(
            "upstream_challenge",
            "ChatGPT 要求浏览器验证，这次没能读到；请到原站确认账号状态",
        ),
        403 => Problem::new(
            "oauth_forbidden",
            "当前凭据无权读取这部分信息，或请求被上游拦截",
        ),
        429 => Problem::new("rate_limited", "ChatGPT 限制了查询频率，请稍后重试"),
        300..=399 => Problem::new("upstream_unavailable", "ChatGPT 返回了跳转，没有读到数据"),
        _ => Problem::new(
            "upstream_unavailable",
            format!("ChatGPT 暂未提供查询结果（HTTP {status}）"),
        ),
    }
}

/// 宿主回调失败归类；不带宿主的原始消息（可能有连接细节）
#[must_use]
pub fn host_problem(error: &PluginFault) -> Problem {
    match error.code {
        ErrorCode::Conflict => Problem::new("credential_changed", "账号凭据刚更新过，请重试"),
        ErrorCode::Rejected => Problem::new("not_found", "CPR 里没有这个账号"),
        ErrorCode::PermissionDenied => {
            Problem::new("account_unavailable", "账号在 CPR 已停用或不可用")
        }
        ErrorCode::Timeout => Problem::new("upstream_timeout", "连接 ChatGPT 超时"),
        ErrorCode::Upstream => Problem::new(
            "upstream_unavailable",
            "无法经账号绑定的代理连接 ChatGPT（不会回退直连）",
        ),
        _ => Problem::new("host_callback", "宿主操作未完成，请稍后重试"),
    }
}

/// ChatGPT 拒绝时的原因字段（`{"detail": …}` 或 `{"error": {"code", "message"}}`），
/// 短而可打印，只进日志
#[must_use]
pub fn refusal_reason(body: &[u8]) -> String {
    let Ok(v) = serde_json::from_slice::<Value>(body) else {
        return String::new();
    };
    let mut parts = Vec::new();
    for field in [v.get("detail"), v.get("error")].into_iter().flatten() {
        match field {
            Value::String(s) => parts.push(s.clone()),
            Value::Object(o) => {
                for k in ["code", "type", "message"] {
                    if let Some(s) = o.get(k).and_then(Value::as_str) {
                        parts.push(s.to_owned());
                    }
                }
            }
            _ => {}
        }
    }
    let joined = parts
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" / ");
    joined
        .chars()
        .filter(|c| !c.is_control())
        .take(200)
        .collect()
}

fn header_safe(s: &str) -> bool {
    !s.is_empty() && s.len() <= 16 * 1024 && s.bytes().all(|b| (32..=126).contains(&b))
}

/// 查询参数编码（只用于插件自己拼的值）
#[must_use]
pub fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_refusals() {
        assert_eq!(
            refusal(401, false, "token_revoked / x").code,
            "token_revoked"
        );
        assert_eq!(refusal(401, false, "").code, "oauth_unauthorized");
        assert_eq!(refusal(403, true, "").code, "upstream_challenge");
        assert_eq!(refusal(403, false, "").code, "oauth_forbidden");
        assert_eq!(refusal(429, false, "").code, "rate_limited");
        assert_eq!(refusal(302, false, "").code, "upstream_unavailable");
        assert!(refusal(401, false, "").stops_query());
        assert!(!refusal(404, false, "").stops_query());
    }

    #[test]
    fn reads_refusal_reason() {
        assert_eq!(
            refusal_reason(br#"{"detail":"token_revoked"}"#),
            "token_revoked"
        );
        assert_eq!(
            refusal_reason(br#"{"error":{"code":"x","message":"bad\nthing"}}"#),
            "x / badthing"
        );
        assert_eq!(refusal_reason(b"<html>"), "");
    }

    #[test]
    fn team_plans() {
        assert_eq!(is_team(Some("Team")), Some(true));
        assert_eq!(is_team(Some("free_workspace")), Some(true));
        assert_eq!(is_team(Some("plus")), Some(false));
        assert_eq!(is_team(Some(" ")), None);
        assert_eq!(is_team(None), None);
    }

    #[test]
    fn encodes_query() {
        assert_eq!(encode("a b/c=d"), "a%20b%2Fc%3Dd");
    }
}

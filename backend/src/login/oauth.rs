//! OpenAI OAuth PKCE 登录状态机、手动 OAuth 授权链接与令牌交换
//!
//! 全部认证与 Sentinel 请求经 [`Egress`] 选定的出口发出：`host.http.proxy`（按代理 ID）
//! 或 `host.http.account`（账号在 CPR 里的实际出口），绝不调用 `host.http.do`。

use std::collections::BTreeMap;

use gateway_plugin_sdk::{
    ErrorCode, PluginFault,
    call::host::{AccountHttpRequest, HttpRequest, ProxyHttpRequest},
    client::HostClient,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::api::parse_query;
use crate::chatgpt::encode;
use crate::clock::now_ms;
use crate::login::sentinel::{
    BrowserProfile, DEFAULT_SENTINEL_ORIGIN, DEFAULT_SENTINEL_REFERER, DEFAULT_SENTINEL_URL,
    Identity, SentinelChallenge, base64_decode, base64_url_encode, build_sentinel_token,
    ensure_identity, fill_random_bytes, requirements_token,
};
use crate::login::totp::totp_code;

pub const DEFAULT_ORIGIN: &str = "https://auth.openai.com";
pub const DEFAULT_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
pub const DEFAULT_SCOPE: &str = "openid profile email offline_access";
pub const DEFAULT_REDIRECT: &str = "http://localhost:1455/auth/callback";
const MAX_RESPONSE_BYTES: usize = 512 * 1024;
const MAX_STEPS: usize = 24;

/// 登录请求的出口
#[derive(Debug, Clone)]
pub enum Egress {
    /// `host.http.proxy`：按代理 ID 现读配置，不依赖任何账号；代理不存在、停用或连不上都失败，不回退直连
    Proxy { proxy_id: String },
    /// `host.http.account`：账号在 CPR 里的实际出口（绑定的代理；没绑或代理停用时直连），
    /// 要求账号启用、凭据版本没变
    Account {
        account_id: String,
        credential_revision: u64,
    },
}

#[derive(Debug, Clone)]
pub struct LoginInput {
    pub email: String,
    pub password: String,
    pub totp_secret: String,
    pub workspace_id: String,
    pub organization_id: String,
    pub expected_workspace_id: Option<String>,
    pub identity: Identity,
    pub redirect_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tokens {
    pub access_token: String,
    pub refresh_token: String,
    pub id_token: String,
    pub identity: TokenIdentity,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenIdentity {
    pub account_id: String,
    pub user_id: String,
    pub email: String,
    pub plan_type: String,
    pub subject: String,
    pub poid: Option<String>,
    pub expires_at_ms: i64,
}

#[derive(Debug, Clone)]
pub struct LoginOutput {
    pub callback_url: String,
    pub tokens: Tokens,
    pub identity: Identity,
    pub identity_changed: bool,
}

#[derive(Debug, Clone)]
pub struct LoginCallbackOutput {
    pub callback_url: String,
    pub code: String,
    pub identity: Identity,
    pub identity_changed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LoginError {
    pub stage: &'static str,
    pub code: &'static str,
    pub message: String,
    /// 多个工作区或组织时可选的候选，页面点一下就带着它重试
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<Choice>,
}

/// 登录时可选的一个工作区（`workspace`）或组织（`organization`）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Choice {
    pub kind: String,
    pub id: String,
    pub name: String,
}

impl LoginError {
    #[must_use]
    pub fn new(stage: &'static str, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            stage,
            code,
            message: message.into(),
            choices: Vec::new(),
        }
    }

    #[must_use]
    fn with_choices(mut self, kind: &'static str, candidates: &[WorkspaceCandidate]) -> Self {
        self.choices = candidates
            .iter()
            .map(|c| Choice {
                kind: kind.to_owned(),
                id: c.id.clone(),
                name: c.name.clone(),
            })
            .collect();
        self
    }
}

/// 30 秒的 TOTP 窗口只剩这么多时，等到下一个窗口再生成验证码
const TOTP_MIN_REMAINING_MS: i64 = 4_000;

/// 离当前 TOTP 窗口结束不到 [`TOTP_MIN_REMAINING_MS`] 时，返回要等多久（多等 300 毫秒越过边界）
#[must_use]
pub fn totp_wait_ms(now_ms: i64) -> u64 {
    let remaining = 30_000 - now_ms.rem_euclid(30_000);
    if remaining <= TOTP_MIN_REMAINING_MS {
        u64::try_from(remaining + 300).unwrap_or(0)
    } else {
        0
    }
}

/// 生成 PKCE 的 `state`、`verifier` 与 `challenge`
///
/// # Errors
/// 随机数不可用时返回错误
pub fn new_pkce() -> Result<(String, String, String), LoginError> {
    let state = random_token_32().map_err(|_| {
        LoginError::new(
            "connect",
            "internal_error",
            "无法生成 OAuth PKCE state 随机数",
        )
    })?;
    let verifier = random_token_32().map_err(|_| {
        LoginError::new(
            "connect",
            "internal_error",
            "无法生成 OAuth PKCE verifier 随机数",
        )
    })?;
    let sum = Sha256::digest(verifier.as_bytes());
    let challenge = base64_url_encode(&sum);
    Ok((state, verifier, challenge))
}

fn random_token_32() -> Result<String, &'static str> {
    let mut b = [0u8; 32];
    fill_random_bytes(&mut b).map_err(|_| "random failed")?;
    Ok(base64_url_encode(&b))
}

/// 构造 OpenAI OAuth `authorize` 地址
#[must_use]
pub fn authorize_url(redirect_url: &str, state: &str, challenge: &str) -> String {
    let redirect = normalize_redirect_url(redirect_url);
    let scope = encode(DEFAULT_SCOPE).replace("%20", "+");
    format!(
        "{DEFAULT_ORIGIN}/oauth/authorize?client_id={}&code_challenge={}&code_challenge_method=S256&redirect_uri={}&response_type=code&scope={scope}&state={}",
        encode(DEFAULT_CLIENT_ID),
        encode(challenge),
        encode(&redirect),
        encode(state),
    )
}

#[must_use]
pub fn normalize_redirect_url(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        DEFAULT_REDIRECT.to_owned()
    } else {
        trimmed.to_owned()
    }
}

/// 从授权链接里读出的回调地址和 `state`
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuthorizeParams {
    pub redirect_uri: Option<String>,
    pub state: String,
}

/// 读授权链接的 `redirect_uri` 和 `state`
///
/// CPR 的 `oauth/start` 给的是桌面登录入口
/// `https://chatgpt.com/codex/desktop-auth?authorize_url=<真正的 authorize 链接>`，
/// 两个参数都在内层链接里；外层没有时就拆开 `authorize_url` 再找
#[must_use]
pub fn authorize_params(auth_url: &str) -> AuthorizeParams {
    let mut url = auth_url.trim().to_owned();
    // 最多拆两层，防止恶意嵌套
    for _ in 0..3 {
        let query = parse_query(url.split_once('?').map_or("", |(_, q)| q));
        let redirect_uri = query
            .get("redirect_uri")
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty());
        let state = query.get("state").map_or("", String::as_str).trim();
        if redirect_uri.is_some() || !state.is_empty() {
            return AuthorizeParams {
                redirect_uri,
                state: state.to_owned(),
            };
        }
        match query.get("authorize_url").map(|s| s.trim()) {
            Some(inner) if !inner.is_empty() => url = inner.to_owned(),
            _ => break,
        }
    }
    AuthorizeParams::default()
}

/// 使用外部提供的 OAuth `authorize` 地址（如 CPR `oauth/start` 返回的 `authorizationUrl`）
/// 执行邮箱 + 密码 + 2FA 登录状态机，截下回调地址后返回（不换取令牌）
///
/// # Errors
/// 任一步骤失败时返回结构化 [`LoginError`]
pub async fn login_with_authorize_url(
    host: &HostClient,
    egress: &Egress,
    input: &LoginInput,
    auth_url: &str,
) -> Result<LoginCallbackOutput, LoginError> {
    let email = input.email.trim().to_ascii_lowercase();
    if email.is_empty() {
        return Err(LoginError::new("identify", "email_missing", "缺少登录邮箱"));
    }
    if input.password.is_empty() {
        return Err(LoginError::new(
            "password",
            "password_missing",
            "缺少登录密码",
        ));
    }
    let params = authorize_params(auth_url);
    let state = params.state;
    let redirect = params
        .redirect_uri
        .unwrap_or_else(|| normalize_redirect_url(&input.redirect_url));
    if !is_valid_callback_base(&redirect) {
        return Err(LoginError::new(
            "connect",
            "invalid_redirect",
            "OAuth 回调地址配置无效",
        ));
    }
    let (identity, profile, identity_changed) = ensure_identity(&input.identity);

    let mut flow = Flow {
        host,
        egress,
        email: &email,
        password: &input.password,
        totp_secret: input.totp_secret.trim(),
        workspace_id: input.workspace_id.trim(),
        organization_id: input.organization_id.trim(),
        expected_workspace_id: input.expected_workspace_id.as_deref(),
        redirect_url: &redirect,
        state: &state,
        profile,
        device_id: &identity.device_id,
        cookies: BTreeMap::from([("oai-did".to_owned(), identity.device_id.clone())]),
        stage: "connect",
    };

    let (callback_url, code) = flow.run_from_url(auth_url).await?;
    Ok(LoginCallbackOutput {
        callback_url,
        code,
        identity,
        identity_changed,
    })
}

/// 执行完整的邮箱 + 密码 + 2FA 登录状态机并换取 OAuth 令牌
///
/// # Errors
/// 任一步骤失败时返回结构化 [`LoginError`]，绝不包含明文密码、2FA 密钥或令牌
pub async fn login_and_exchange(
    host: &HostClient,
    egress: &Egress,
    input: &LoginInput,
) -> Result<LoginOutput, LoginError> {
    let email = input.email.trim().to_ascii_lowercase();
    if email.is_empty() {
        return Err(LoginError::new("identify", "email_missing", "缺少登录邮箱"));
    }
    if input.password.is_empty() {
        return Err(LoginError::new(
            "password",
            "password_missing",
            "缺少登录密码",
        ));
    }
    let redirect = normalize_redirect_url(&input.redirect_url);
    if !is_valid_callback_base(&redirect) {
        return Err(LoginError::new(
            "connect",
            "invalid_redirect",
            "OAuth 回调地址配置无效",
        ));
    }
    let (identity, profile, identity_changed) = ensure_identity(&input.identity);
    let (state, verifier, challenge) = new_pkce()?;
    let auth_url = authorize_url(&redirect, &state, &challenge);

    let mut flow = Flow {
        host,
        egress,
        email: &email,
        password: &input.password,
        totp_secret: input.totp_secret.trim(),
        workspace_id: input.workspace_id.trim(),
        organization_id: input.organization_id.trim(),
        expected_workspace_id: input.expected_workspace_id.as_deref(),
        redirect_url: &redirect,
        state: &state,
        profile,
        device_id: &identity.device_id,
        cookies: BTreeMap::from([("oai-did".to_owned(), identity.device_id.clone())]),
        stage: "connect",
    };

    let (callback_url, code) = flow.run_from_url(&auth_url).await?;
    let tokens = exchange_authorization_code(
        host,
        egress,
        &redirect,
        &code,
        &verifier,
        &email,
        &flow.profile,
    )
    .await?;

    Ok(LoginOutput {
        callback_url,
        tokens,
        identity,
        identity_changed,
    })
}

/// 用授权码换取 OAuth 令牌（支持状态机自动登录与手动粘贴回调 URL 两种入口）
///
/// # Errors
/// 网络、挑战、上游拒绝或令牌身份不匹配时返回 [`LoginError`]
pub async fn exchange_authorization_code(
    host: &HostClient,
    egress: &Egress,
    redirect_url: &str,
    code: &str,
    verifier: &str,
    expected_email: &str,
    profile: &BrowserProfile,
) -> Result<Tokens, LoginError> {
    let redirect = normalize_redirect_url(redirect_url);
    let form = format!(
        "grant_type=authorization_code&client_id={}&redirect_uri={}&code={}&code_verifier={}",
        encode(DEFAULT_CLIENT_ID),
        encode(&redirect),
        encode(code.trim()),
        encode(verifier.trim()),
    );
    let mut headers = vec![
        (
            "Content-Type".into(),
            b"application/x-www-form-urlencoded".to_vec(),
        ),
        ("Accept".into(), b"application/json".to_vec()),
    ];
    headers.extend(profile.headers());

    let resp = raw_http(
        host,
        egress,
        "exchange",
        "POST",
        &format!("{DEFAULT_ORIGIN}/oauth/token"),
        headers,
        form.into_bytes(),
    )
    .await?;

    if !(200..300).contains(&resp.status) {
        return Err(classify_http_status(
            "exchange",
            resp.status,
            &resp.headers,
            &resp.body,
            true,
        ));
    }

    #[derive(Deserialize)]
    struct TokenPayload {
        #[serde(default)]
        access_token: String,
        #[serde(default)]
        refresh_token: String,
        #[serde(default)]
        id_token: String,
    }

    let payload: TokenPayload = serde_json::from_slice(&resp.body).map_err(|_| {
        LoginError::new(
            "exchange",
            "invalid_response",
            "令牌交换响应不是有效的 JSON",
        )
    })?;
    if payload.access_token.trim().is_empty() || payload.refresh_token.trim().is_empty() {
        return Err(LoginError::new(
            "exchange",
            "incomplete_tokens",
            "OpenAI 返回的令牌不完整（缺少 access_token 或 refresh_token）",
        ));
    }

    let identity = parse_token_identity(&payload.access_token, &payload.id_token, now_ms())?;
    let expected = expected_email.trim();
    if !expected.is_empty()
        && !identity.email.is_empty()
        && !identity.email.eq_ignore_ascii_case(expected)
    {
        return Err(LoginError::new(
            "exchange",
            "identity_mismatch",
            "登录返回的账号邮箱与目标账号邮箱不一致",
        ));
    }

    Ok(Tokens {
        access_token: payload.access_token,
        refresh_token: payload.refresh_token,
        id_token: payload.id_token,
        identity,
    })
}

/// 校验并提取回调 URL 中的 `(callback_url, code)`
///
/// # Errors
/// 不是合法回调地址、`state` 不匹配或缺少 `code` 时返回错误
pub fn parse_callback_url(
    raw_url: &str,
    redirect_url: &str,
    expected_state: &str,
) -> Result<(String, String), LoginError> {
    let trimmed = raw_url.trim();
    let redirect = normalize_redirect_url(redirect_url);
    if !matches_callback_base(trimmed, &redirect) {
        return Err(LoginError::new(
            "authorize",
            "invalid_callback",
            "回调链接的地址、端口或路径与当前 OAuth 配置不匹配",
        ));
    }
    let query_str = trimmed.split_once('?').map_or("", |(_, q)| q);
    if query_str.contains('#') {
        return Err(LoginError::new(
            "authorize",
            "invalid_callback",
            "回调链接不能包含 URL 片段（#）",
        ));
    }
    let query = parse_query(query_str);
    if query.get("error").is_some_and(|e| !e.trim().is_empty()) {
        return Err(LoginError::new(
            "authorize",
            "invalid_callback",
            "回调链接中包含上游返回的授权错误",
        ));
    }
    let state = query.get("state").map_or("", String::as_str).trim();
    let code = query.get("code").map_or("", String::as_str).trim();
    if state != expected_state || code.is_empty() {
        return Err(LoginError::new(
            "authorize",
            "invalid_callback",
            "回调链接中的 state 不匹配或缺少授权码 code",
        ));
    }
    Ok((trimmed.to_owned(), code.to_owned()))
}

struct Flow<'a> {
    host: &'a HostClient,
    egress: &'a Egress,
    email: &'a str,
    password: &'a str,
    totp_secret: &'a str,
    workspace_id: &'a str,
    organization_id: &'a str,
    expected_workspace_id: Option<&'a str>,
    redirect_url: &'a str,
    state: &'a str,
    profile: BrowserProfile,
    device_id: &'a str,
    cookies: BTreeMap<String, String>,
    stage: &'static str,
}

#[derive(Debug, Default)]
struct Page {
    kind: String,
    next: String,
    data: Map<String, Value>,
}

impl Flow<'_> {
    async fn run_from_url(&mut self, auth_url: &str) -> Result<(String, String), LoginError> {
        self.stage = "connect";
        let mut p = self.get(auth_url, "").await?;
        for _ in 0..MAX_STEPS {
            if p.next.is_empty() {
                break;
            }
            if matches_callback_base(&p.next, self.redirect_url) {
                return Err(LoginError::new(
                    self.stage,
                    "unexpected_callback",
                    "尚未提交凭据即收到 OAuth 回调跳转",
                ));
            }
            p = self.get(&p.next, auth_url).await?;
        }

        self.stage = "identify";
        let referer = format!("{DEFAULT_ORIGIN}/log-in");
        p = self
            .post_json(
                "/api/accounts/authorize/continue",
                &referer,
                "authorize_continue",
                &json!({"username": {"kind": "email", "value": self.email}}),
            )
            .await?;

        let mut password_sent = false;
        let mut totp_sent = false;

        for _ in 0..MAX_STEPS {
            if let Some(res) = self.check_callback(&p.next) {
                return res;
            }
            match p.kind.as_str() {
                "password" | "login_password" => {
                    if password_sent {
                        return Err(LoginError::new(
                            "password",
                            "password_rejected",
                            "邮箱或密码不正确，OpenAI 拒绝了登录密码",
                        ));
                    }
                    self.stage = "password";
                    password_sent = true;
                    let pw_ref = format!("{DEFAULT_ORIGIN}/log-in/password");
                    p = self
                        .post_json(
                            "/api/accounts/password/verify",
                            &pw_ref,
                            "password_verify",
                            &json!({"password": self.password}),
                        )
                        .await?;
                }
                "mfa_challenge" | "mfa-challenge" => {
                    if totp_sent {
                        return Err(LoginError::new(
                            "totp",
                            "totp_rejected",
                            "2FA 动态验证码被拒绝，请检查 2FA 密钥是否正确",
                        ));
                    }
                    self.stage = "totp";
                    totp_sent = true;
                    p = self.verify_totp(&p).await?;
                }
                "consent" | "workspace" | "workspace_select" => {
                    self.stage = "authorize";
                    p = self.select_workspace(&p).await?;
                }
                "organization_select" => {
                    self.stage = "authorize";
                    p = self.select_organization(&p.data, "").await?;
                }
                "email_otp" | "email_otp_verification" | "email-verification" => {
                    return Err(LoginError::new(
                        self.stage,
                        "email_otp_required",
                        "该账号要求邮箱验证码（Email OTP），无法仅凭密码和 2FA 自动登录，请使用手动 OAuth 链接登录",
                    ));
                }
                "add_phone" | "phone" | "phone_verification" | "captcha" | "sso" | "signup" => {
                    return Err(LoginError::new(
                        self.stage,
                        "unsupported_challenge",
                        format!(
                            "遇到不支持的附加验证步骤（{}），请使用手动 OAuth 链接登录",
                            p.kind
                        ),
                    ));
                }
                _ => {
                    if p.next.is_empty() {
                        return Err(LoginError::new(
                            self.stage,
                            "unsupported_login_step",
                            "无法识别 OpenAI 返回的下一步登录状态",
                        ));
                    }
                    if let Some(res) = self.check_callback(&p.next) {
                        return res;
                    }
                    p = self.get(&p.next, DEFAULT_ORIGIN).await?;
                }
            }
        }
        Err(LoginError::new(
            self.stage,
            "too_many_steps",
            "登录跳转步骤过多，已中止",
        ))
    }

    fn check_callback(&self, candidate: &str) -> Option<Result<(String, String), LoginError>> {
        if candidate.is_empty() || !matches_callback_base(candidate, self.redirect_url) {
            return None;
        }
        Some(
            parse_callback_url(candidate, self.redirect_url, self.state).map_err(|mut e| {
                e.stage = self.stage;
                e
            }),
        )
    }

    async fn verify_totp(&mut self, p: &Page) -> Result<Page, LoginError> {
        let mut factor_id = find_totp_factor_id(&p.data);
        if factor_id.is_empty() {
            let sess = self.session().await?;
            factor_id = find_totp_factor_id(&sess);
        }
        if factor_id.is_empty() {
            return Err(LoginError::new(
                "totp",
                "unsupported_mfa",
                "账号开启了非 TOTP 的多因素验证，无法使用 2FA 密钥完成验证",
            ));
        }
        let mfa_ref = format!("{DEFAULT_ORIGIN}/mfa-challenge");
        let _ = self
            .post_json(
                "/api/accounts/mfa/issue_challenge",
                &mfa_ref,
                "",
                &json!({
                    "id": factor_id,
                    "type": "totp",
                    "force_fresh_challenge": false,
                }),
            )
            .await
            .map_err(|e| {
                // 这一步还没提交验证码，被拒不代表 2FA 密钥不对
                if matches!(e.code, "login_rejected" | "upstream_rejected") {
                    LoginError::new(
                        "totp",
                        "mfa_issue_failed",
                        format!("OpenAI 没能发起 2FA 验证：{}", e.message),
                    )
                } else {
                    e
                }
            })?;
        let wait = totp_wait_ms(now_ms());
        if wait > 0 {
            // 验证码临近失效，加上代理延迟可能到达时已过期
            tokio::time::sleep(std::time::Duration::from_millis(wait)).await;
        }
        let code = totp_code(self.totp_secret, now_ms()).map_err(|err_code| {
            let msg = if err_code == "totp_secret_missing" {
                "该账号开启了 2FA 验证，但尚未提供 2FA 密钥"
            } else {
                "提供的 2FA 密钥格式无效（需为 Base32 密钥或 6位/30秒/SHA1 的 otpauth 链接）"
            };
            LoginError::new("totp", err_code, msg)
        })?;
        self.post_json(
            "/api/accounts/mfa/verify",
            &mfa_ref,
            "",
            &json!({
                "id": factor_id,
                "type": "totp",
                "code": code,
            }),
        )
        .await
        .map_err(|e| {
            if matches!(e.code, "login_rejected" | "upstream_rejected") {
                LoginError::new(
                    "totp",
                    "totp_rejected",
                    format!(
                        "2FA 动态验证码被拒绝，请检查 2FA 密钥是否正确（{}）",
                        e.message
                    ),
                )
            } else {
                e
            }
        })
    }

    async fn select_workspace(&mut self, p: &Page) -> Result<Page, LoginError> {
        let mut sess = self.session().await.unwrap_or_default();
        if sess.is_empty() {
            sess = p.data.clone();
        }
        let direct_ws = string_field(&sess, "workspace_id");
        let candidates = workspace_candidates(&sess);
        let chosen_id = if !self.workspace_id.is_empty() {
            if candidates.is_empty() {
                self.workspace_id.to_owned()
            } else if let Some(found) = candidates.iter().find(|c| c.id == self.workspace_id) {
                found.id.clone()
            } else {
                return Err(LoginError::new(
                    "authorize",
                    "workspace_unavailable",
                    "指定的工作区不在这个账号可用的工作区里，请重选",
                )
                .with_choices("workspace", &candidates));
            }
        } else if !direct_ws.is_empty() {
            direct_ws
        } else if let Some(expected) = self
            .expected_workspace_id
            .filter(|id| !id.trim().is_empty())
            && let Some(found) = candidates.iter().find(|c| c.id == expected.trim())
        {
            found.id.clone()
        } else {
            match candidates.len() {
                0 => {
                    return Err(LoginError::new(
                        "authorize",
                        "workspace_unavailable",
                        "未找到可用的 ChatGPT Workspace",
                    ));
                }
                1 => candidates[0].id.clone(),
                _ => {
                    let summary = candidates
                        .iter()
                        .map(|c| {
                            if c.name.is_empty() {
                                c.id.as_str()
                            } else {
                                c.name.as_str()
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("、");
                    return Err(LoginError::new(
                        "authorize",
                        "workspace_ambiguous",
                        format!("账号有 {} 个工作区：{summary}", candidates.len()),
                    )
                    .with_choices("workspace", &candidates));
                }
            }
        };

        let consent_ref = format!("{DEFAULT_ORIGIN}/sign-in-with-chatgpt/codex/consent");
        let next = self
            .post_json(
                "/api/accounts/workspace/select",
                &consent_ref,
                "",
                &json!({"workspace_id": chosen_id}),
            )
            .await?;
        if next.kind == "organization_select" || !organization_candidates(&next.data).is_empty() {
            return self.select_organization(&next.data, &consent_ref).await;
        }
        Ok(next)
    }

    async fn select_organization(
        &mut self,
        data: &Map<String, Value>,
        referer: &str,
    ) -> Result<Page, LoginError> {
        let candidates = organization_candidates(data);
        let chosen = if !self.organization_id.is_empty() {
            candidates
                .iter()
                .find(|c| c.id == self.organization_id)
                .cloned()
                .ok_or_else(|| {
                    LoginError::new(
                        "authorize",
                        "workspace_unavailable",
                        "指定的组织不在可选列表中，请重选",
                    )
                    .with_choices("organization", &candidates)
                })?
        } else {
            match candidates.len() {
                0 => {
                    return Err(LoginError::new(
                        "authorize",
                        "workspace_unavailable",
                        "未找到可用的 Organization",
                    ));
                }
                1 => candidates[0].clone(),
                _ => {
                    return Err(LoginError::new(
                        "authorize",
                        "workspace_ambiguous",
                        "账号有多个组织，请选一个",
                    )
                    .with_choices("organization", &candidates));
                }
            }
        };
        let mut body = json!({"org_id": chosen.id});
        if !chosen.project_id.is_empty() {
            body["project_id"] = json!(chosen.project_id);
        }
        let org_ref = if referer.is_empty() {
            format!("{DEFAULT_ORIGIN}/sign-in-with-chatgpt/codex/consent")
        } else {
            referer.to_owned()
        };
        self.post_json("/api/accounts/organization/select", &org_ref, "", &body)
            .await
    }

    async fn session(&mut self) -> Result<Map<String, Value>, LoginError> {
        if let Some(raw_cookie) = self.cookies.get("oai-client-auth-session")
            && let Some(data) = decode_session_cookie(raw_cookie)
        {
            return Ok(data);
        }
        let referer = format!("{DEFAULT_ORIGIN}/sign-in-with-chatgpt/codex/consent");
        let page = self
            .get(
                &format!("{DEFAULT_ORIGIN}/api/accounts/client_auth_session_dump"),
                &referer,
            )
            .await?;
        if let Some(Value::Object(inner)) = page.data.get("client_auth_session") {
            return Ok(inner.clone());
        }
        if page.data.is_empty() {
            return Err(LoginError::new(
                self.stage,
                "invalid_response",
                "无法读取登录会话的工作区信息",
            ));
        }
        Ok(page.data)
    }

    async fn get(&mut self, target: &str, referer: &str) -> Result<Page, LoginError> {
        self.request("GET", target, referer, "", None).await
    }

    async fn post_json(
        &mut self,
        path: &str,
        referer: &str,
        sentinel_flow: &str,
        body: &Value,
    ) -> Result<Page, LoginError> {
        let raw = serde_json::to_vec(body).map_err(|_| {
            LoginError::new(self.stage, "invalid_response", "请求正文 JSON 编码失败")
        })?;
        let target = format!("{DEFAULT_ORIGIN}{path}");
        self.request("POST", &target, referer, sentinel_flow, Some(raw))
            .await
    }

    async fn request(
        &mut self,
        method: &str,
        target: &str,
        referer: &str,
        sentinel_flow: &str,
        body: Option<Vec<u8>>,
    ) -> Result<Page, LoginError> {
        let mut headers = Vec::new();
        if body.is_some() {
            headers.push(("Content-Type".into(), b"application/json".to_vec()));
            headers.push(("Accept".into(), b"application/json".to_vec()));
            headers.push(("Origin".into(), DEFAULT_ORIGIN.as_bytes().to_vec()));
        } else {
            headers.push((
                "Accept".into(),
                b"text/html,application/xhtml+xml,application/json;q=0.9,*/*;q=0.8".to_vec(),
            ));
        }
        if !referer.is_empty() {
            headers.push(("Referer".into(), referer.as_bytes().to_vec()));
        }
        headers.extend(self.profile.headers());
        if !sentinel_flow.is_empty()
            && let Some(token) = self.sentinel_token(sentinel_flow).await
        {
            headers.push(("Openai-Sentinel-Token".into(), token.into_bytes()));
        }
        if !self.cookies.is_empty() {
            let cookie_line = self
                .cookies
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join("; ");
            headers.push(("Cookie".into(), cookie_line.into_bytes()));
        }

        let resp = raw_http(
            self.host,
            self.egress,
            self.stage,
            method,
            target,
            headers,
            body.unwrap_or_default(),
        )
        .await?;

        self.record_cookies(&resp.headers);

        if (300..400).contains(&resp.status) {
            let loc = header_value(&resp.headers, "location").unwrap_or_default();
            let resolved = resolve_url(&loc, self.redirect_url, self.stage)?;
            return Ok(Page {
                kind: String::new(),
                next: resolved,
                data: Map::new(),
            });
        }

        if !(200..300).contains(&resp.status) {
            return Err(classify_http_status(
                self.stage,
                resp.status,
                &resp.headers,
                &resp.body,
                false,
            ));
        }

        if resp.body.iter().all(u8::is_ascii_whitespace) {
            return Ok(Page {
                kind: page_kind_from_url(target),
                next: String::new(),
                data: Map::new(),
            });
        }

        let Ok(Value::Object(data)) = serde_json::from_slice::<Value>(&resp.body) else {
            return Ok(Page {
                kind: page_kind_from_url(target),
                next: String::new(),
                data: Map::new(),
            });
        };

        let mut p = Page {
            kind: nested_string(&data, &["page", "type"])
                .or_else(|| {
                    data.get("page_type")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                })
                .unwrap_or_default()
                .trim()
                .to_owned(),
            next: data
                .get("continue_url")
                .or_else(|| data.get("url"))
                .and_then(Value::as_str)
                .map(str::to_owned)
                .or_else(|| nested_string(&data, &["page", "payload", "url"]))
                .unwrap_or_default()
                .trim()
                .to_owned(),
            data,
        };
        if !p.next.is_empty() {
            p.next = resolve_url(&p.next, self.redirect_url, self.stage)?;
        }
        if p.kind.is_empty() {
            p.kind = if !p.next.is_empty() {
                page_kind_from_url(&p.next)
            } else {
                page_kind_from_url(target)
            };
        }
        Ok(p)
    }

    async fn sentinel_token(&self, flow_name: &str) -> Option<String> {
        let req_body = json!({
            "p": requirements_token(&self.profile, self.device_id),
            "id": self.device_id,
            "flow": flow_name,
        });
        let raw_body = serde_json::to_vec(&req_body).ok()?;
        let mut headers = vec![
            ("Content-Type".into(), b"text/plain;charset=UTF-8".to_vec()),
            ("Accept".into(), b"application/json".to_vec()),
            ("Origin".into(), DEFAULT_SENTINEL_ORIGIN.as_bytes().to_vec()),
            (
                "Referer".into(),
                DEFAULT_SENTINEL_REFERER.as_bytes().to_vec(),
            ),
            ("oai-device-id".into(), self.device_id.as_bytes().to_vec()),
        ];
        headers.extend(self.profile.headers());
        let resp = raw_http(
            self.host,
            self.egress,
            self.stage,
            "POST",
            DEFAULT_SENTINEL_URL,
            headers,
            raw_body,
        )
        .await
        .ok()?;
        if !(200..300).contains(&resp.status) {
            return None;
        }
        let challenge: SentinelChallenge = serde_json::from_slice(&resp.body).ok()?;
        build_sentinel_token(&self.profile, self.device_id, flow_name, &challenge)
    }

    fn record_cookies(&mut self, headers: &[(String, Vec<u8>)]) {
        for (k, v) in headers {
            if !k.eq_ignore_ascii_case("set-cookie") {
                continue;
            }
            let Ok(raw) = std::str::from_utf8(v) else {
                continue;
            };
            let first = raw.split(';').next().unwrap_or_default().trim();
            let Some((name, val)) = first.split_once('=') else {
                continue;
            };
            let name = name.trim();
            let val = val.trim();
            if name.is_empty() {
                continue;
            }
            if val.is_empty() {
                self.cookies.remove(name);
            } else {
                self.cookies.insert(name.to_owned(), val.to_owned());
            }
        }
    }
}

struct RawHttpReply {
    status: u16,
    headers: Vec<(String, Vec<u8>)>,
    body: Vec<u8>,
}

async fn raw_http(
    host: &HostClient,
    egress: &Egress,
    stage: &'static str,
    method: &str,
    url: &str,
    headers: Vec<(String, Vec<u8>)>,
    body: Vec<u8>,
) -> Result<RawHttpReply, LoginError> {
    let request = HttpRequest {
        method: method.to_owned(),
        url: url.to_owned(),
        headers,
    };
    let response = match egress {
        Egress::Proxy { proxy_id } => {
            host.proxy_http(
                ProxyHttpRequest {
                    proxy_id: proxy_id.clone(),
                    request,
                },
                body,
            )
            .await
        }
        Egress::Account {
            account_id,
            credential_revision,
        } => {
            host.account_http(
                AccountHttpRequest {
                    account_id: account_id.clone(),
                    credential_revision: *credential_revision,
                    request,
                },
                body,
            )
            .await
        }
    }
    .map_err(|e| map_host_fault(egress, stage, &e))?;
    let status = response.status;
    let resp_headers = response.headers;
    let body_bytes = response
        .body
        .collect(MAX_RESPONSE_BYTES + 1)
        .await
        .map_err(|e| {
            if e.code == ErrorCode::Capacity {
                LoginError::new(stage, "invalid_response", "上游返回的响应体积超过限制")
            } else {
                map_host_fault(egress, stage, &e)
            }
        })?;
    Ok(RawHttpReply {
        status,
        headers: resp_headers,
        body: body_bytes,
    })
}

fn map_host_fault(egress: &Egress, stage: &'static str, fault: &PluginFault) -> LoginError {
    if fault.code == ErrorCode::Timeout {
        return LoginError::new(stage, "network_timeout", "连接 OpenAI 认证服务超时");
    }
    match egress {
        // 宿主按 ID 解析代理：停用或 ID 不合法是 InvalidInput，不存在是 Rejected，连不上是 Upstream
        Egress::Proxy { .. } => match fault.code {
            ErrorCode::InvalidInput => LoginError::new(
                stage,
                "proxy_unavailable",
                "这个代理在 CPR 里已停用，请换一个",
            ),
            ErrorCode::Rejected => LoginError::new(
                stage,
                "proxy_unavailable",
                "CPR 里没有这个代理，可能刚被删除，请刷新后重选",
            ),
            ErrorCode::Upstream => LoginError::new(
                stage,
                "source_proxy_unavailable",
                "经所选代理连不上 OpenAI（不会回退直连），请先在 CPR 测试这个代理",
            ),
            _ => LoginError::new(stage, "network", "宿主网络请求未完成，请稍后重试"),
        },
        Egress::Account { .. } => match fault.code {
            ErrorCode::Upstream => LoginError::new(
                stage,
                "source_proxy_unavailable",
                "无法经账号绑定的代理连接 OpenAI（代理不可用，不会回退直连）",
            ),
            ErrorCode::PermissionDenied => {
                LoginError::new(stage, "account_unavailable", "账号在 CPR 里已停用或不可用")
            }
            ErrorCode::Conflict => LoginError::new(
                stage,
                "credential_changed",
                "账号凭据刚被 CPR 更新过，请重试",
            ),
            ErrorCode::Rejected => {
                LoginError::new(stage, "account_unavailable", "CPR 里找不到这个账号")
            }
            _ => LoginError::new(stage, "network", "宿主网络请求未完成，请稍后重试"),
        },
    }
}

fn classify_http_status(
    stage: &'static str,
    status: u16,
    headers: &[(String, Vec<u8>)],
    body: &[u8],
    is_token_exchange: bool,
) -> LoginError {
    if is_cloudflare_challenge(headers, body) {
        return LoginError::new(
            stage,
            "browser_challenge",
            "遇到 Cloudflare 浏览器验证拦截，请使用手动 OAuth 授权链接完成登录",
        );
    }
    if status == 429 {
        return LoginError::new(
            stage,
            "rate_limited",
            "请求过于频繁（HTTP 429），请稍后重试",
        );
    }
    if is_token_exchange {
        return LoginError::new(
            stage,
            "token_exchange_failed",
            format!("授权码换取令牌失败（HTTP {status}）"),
        );
    }
    match status {
        401 => LoginError::new(stage, "login_rejected", "认证请求被拒绝（HTTP 401）"),
        403 => LoginError::new(stage, "login_forbidden", "认证请求被禁止（HTTP 403）"),
        _ => LoginError::new(
            stage,
            "upstream_rejected",
            format!("OpenAI 认证服务返回错误（HTTP {status}）"),
        ),
    }
}

fn is_cloudflare_challenge(headers: &[(String, Vec<u8>)], body: &[u8]) -> bool {
    if headers.iter().any(|(k, v)| {
        k.eq_ignore_ascii_case("cf-mitigated")
            && std::str::from_utf8(v)
                .unwrap_or_default()
                .to_ascii_lowercase()
                .contains("challenge")
    }) {
        return true;
    }
    let text = String::from_utf8_lossy(body).to_ascii_lowercase();
    text.contains("cf-chl-") || text.contains("challenges.cloudflare.com")
}

fn header_value(headers: &[(String, Vec<u8>)], name: &str) -> Option<String> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .and_then(|(_, v)| std::str::from_utf8(v).ok())
        .map(|s| s.trim().to_owned())
}

fn resolve_url(raw: &str, redirect_url: &str, stage: &'static str) -> Result<String, LoginError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(LoginError::new(
            stage,
            "invalid_redirect",
            "上游返回了空的跳转地址",
        ));
    }
    let full = if trimmed.starts_with('/') {
        format!("{DEFAULT_ORIGIN}{trimmed}")
    } else {
        trimmed.to_owned()
    };
    if matches_callback_base(&full, redirect_url) {
        return Ok(full);
    }
    if full.starts_with("https://auth.openai.com/")
        || full == "https://auth.openai.com"
        || full.starts_with("https://chatgpt.com/")
    {
        return Ok(full);
    }
    if full.starts_with("http://") || full.starts_with("https://") {
        return Err(LoginError::new(
            stage,
            "unsupported_redirect",
            "上游跳转到不受信任的外部域名，已拦截",
        ));
    }
    Err(LoginError::new(
        stage,
        "invalid_redirect",
        "上游跳转地址格式无效",
    ))
}

fn is_valid_callback_base(redirect_url: &str) -> bool {
    let Some((scheme, rest)) = redirect_url.trim().split_once("://") else {
        return false;
    };
    if !matches!(scheme, "http" | "https") || rest.contains('#') || rest.contains('@') {
        return false;
    }
    let host_and_path = rest.split('?').next().unwrap_or(rest);
    let (host, _) = host_and_path.split_once('/').unwrap_or((host_and_path, ""));
    !host.is_empty()
}

fn matches_callback_base(candidate: &str, redirect_url: &str) -> bool {
    let Some((c_scheme, c_rest)) = candidate.trim().split_once("://") else {
        return false;
    };
    let Some((r_scheme, r_rest)) = redirect_url.trim().split_once("://") else {
        return false;
    };
    if !c_scheme.eq_ignore_ascii_case(r_scheme) || c_rest.contains('#') || c_rest.contains('@') {
        return false;
    }
    let c_no_query = c_rest.split('?').next().unwrap_or(c_rest);
    let r_no_query = r_rest.split('?').next().unwrap_or(r_rest);
    let (c_host, c_path) = c_no_query.split_once('/').unwrap_or((c_no_query, ""));
    let (r_host, r_path) = r_no_query.split_once('/').unwrap_or((r_no_query, ""));
    c_host.eq_ignore_ascii_case(r_host)
        && c_path.trim_end_matches('/') == r_path.trim_end_matches('/')
}

fn page_kind_from_url(raw_url: &str) -> String {
    let without_query = raw_url.split('?').next().unwrap_or(raw_url);
    let path = if let Some((_, rest)) = without_query.split_once("://") {
        rest.find('/').map_or("/", |idx| &rest[idx..])
    } else {
        without_query
    };
    match path.trim_end_matches('/') {
        "/log-in/password" => "password".to_owned(),
        "/email-verification" => "email_otp".to_owned(),
        "/mfa-challenge" => "mfa_challenge".to_owned(),
        "/sign-in-with-chatgpt/codex/consent" => "consent".to_owned(),
        _ => String::new(),
    }
}

fn find_totp_factor_id(data: &Map<String, Value>) -> String {
    for key in ["mfa_factors", "factors"] {
        let Some(Value::Array(items)) = data.get(key) else {
            continue;
        };
        for item in items {
            let Some(obj) = item.as_object() else {
                continue;
            };
            let kind = string_field(obj, "factor_type");
            let kind = if kind.is_empty() {
                string_field(obj, "type")
            } else {
                kind
            };
            if kind.eq_ignore_ascii_case("totp") {
                let id = string_field(obj, "id");
                if !id.is_empty() {
                    return id;
                }
            }
        }
    }
    String::new()
}

#[derive(Debug, Clone)]
struct WorkspaceCandidate {
    id: String,
    name: String,
    project_id: String,
}

fn workspace_candidates(data: &Map<String, Value>) -> Vec<WorkspaceCandidate> {
    let Some(Value::Array(items)) = data.get("workspaces") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for item in items {
        let Some(obj) = item.as_object() else {
            continue;
        };
        let id = string_field(obj, "id");
        if id.is_empty() {
            continue;
        }
        let mut name = string_field(obj, "name");
        if name.is_empty() {
            name = string_field(obj, "title");
        }
        out.push(WorkspaceCandidate {
            id,
            name,
            project_id: String::new(),
        });
    }
    out
}

fn organization_candidates(data: &Map<String, Value>) -> Vec<WorkspaceCandidate> {
    for key in ["orgs", "organizations"] {
        let Some(Value::Array(items)) = data.get(key) else {
            continue;
        };
        let mut out = Vec::new();
        for item in items {
            let Some(obj) = item.as_object() else {
                continue;
            };
            let id = string_field(obj, "id");
            if id.is_empty() {
                continue;
            }
            let mut name = string_field(obj, "name");
            if name.is_empty() {
                name = string_field(obj, "title");
            }
            let project_id = single_project_id(obj);
            out.push(WorkspaceCandidate {
                id,
                name,
                project_id,
            });
        }
        return out;
    }
    Vec::new()
}

fn single_project_id(org: &Map<String, Value>) -> String {
    let Some(Value::Array(projects)) = org.get("projects") else {
        return String::new();
    };
    let mut found = String::new();
    for item in projects {
        let Some(obj) = item.as_object() else {
            continue;
        };
        let id = string_field(obj, "id");
        if id.is_empty() {
            continue;
        }
        if !found.is_empty() {
            return String::new();
        }
        found = id;
    }
    found
}

fn decode_session_cookie(raw_value: &str) -> Option<Map<String, Value>> {
    let unescaped = percent_decode_str(raw_value.trim());
    let cleaned = unescaped
        .trim_matches('"')
        .strip_prefix("s:")
        .unwrap_or(unescaped.trim_matches('"'));
    let mut candidates = vec![cleaned];
    candidates.extend(cleaned.split('.'));
    for part in candidates {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(decoded) = base64_decode(trimmed) else {
            continue;
        };
        let Ok(Value::Object(obj)) = serde_json::from_slice::<Value>(&decoded) else {
            continue;
        };
        if let Some(Value::Object(inner)) = obj.get("client_auth_session") {
            return Some(inner.clone());
        }
        if obj.contains_key("workspaces")
            || obj.contains_key("workspace_id")
            || obj.contains_key("mfa_factors")
            || obj.contains_key("factors")
            || obj.contains_key("orgs")
            || obj.contains_key("organizations")
        {
            return Some(obj);
        }
    }
    None
}

fn percent_decode_str(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(hex) = std::str::from_utf8(&bytes[i + 1..i + 3])
            && let Ok(val) = u8::from_str_radix(hex, 16)
        {
            out.push(val);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn nested_string(data: &Map<String, Value>, path: &[&str]) -> Option<String> {
    let mut current = data;
    for (idx, &segment) in path.iter().enumerate() {
        let val = current.get(segment)?;
        if idx + 1 == path.len() {
            return val.as_str().map(str::to_owned);
        }
        current = val.as_object()?;
    }
    None
}

fn string_field(data: &Map<String, Value>, key: &str) -> String {
    data.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_owned()
}

/// 解析 `access_token`（及可选 `id_token`）中的账号与用户身份信息
///
/// # Errors
/// JWT 格式无效、缺少 `chatgpt_account_id` 或已过期时返回 `invalid_token_identity`
pub fn parse_token_identity(
    access_token: &str,
    id_token: &str,
    now_unix_ms: i64,
) -> Result<TokenIdentity, LoginError> {
    let claims = decode_jwt_claims(access_token).map_err(|_| {
        LoginError::new(
            "exchange",
            "invalid_token_identity",
            "返回的 access_token 不是有效的 JWT",
        )
    })?;
    let auth = claims
        .get("https://api.openai.com/auth")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            LoginError::new(
                "exchange",
                "invalid_token_identity",
                "access_token 中缺少 OpenAI auth 声明",
            )
        })?;
    let account_id = string_field(auth, "chatgpt_account_id");
    let mut user_id = string_field(auth, "chatgpt_user_id");
    if user_id.is_empty() {
        user_id = string_field(auth, "user_id");
    }
    let plan_type = string_field(auth, "chatgpt_plan_type").to_ascii_lowercase();
    let poid = {
        let s = string_field(auth, "poid");
        if s.is_empty() { None } else { Some(s) }
    };

    let mut email = string_field(&claims, "email").to_ascii_lowercase();
    if email.is_empty()
        && let Some(profile) = claims
            .get("https://api.openai.com/profile")
            .and_then(Value::as_object)
    {
        email = string_field(profile, "email").to_ascii_lowercase();
    }
    let mut subject = string_field(&claims, "sub");

    if (email.is_empty() || subject.is_empty())
        && !id_token.trim().is_empty()
        && let Ok(id_claims) = decode_jwt_claims(id_token)
    {
        if email.is_empty() {
            email = string_field(&id_claims, "email").to_ascii_lowercase();
        }
        if subject.is_empty() {
            subject = string_field(&id_claims, "sub");
        }
    }

    let exp_secs = claims
        .get("exp")
        .and_then(|v| v.as_i64().or_else(|| v.as_f64().map(|f| f as i64)))
        .unwrap_or(0);
    let expires_at_ms = exp_secs.saturating_mul(1000);
    if account_id.is_empty() || expires_at_ms <= now_unix_ms {
        return Err(LoginError::new(
            "exchange",
            "invalid_token_identity",
            "access_token 缺少 ChatGPT 账号 ID 或令牌已过期",
        ));
    }

    Ok(TokenIdentity {
        account_id,
        user_id,
        email,
        plan_type,
        subject,
        poid,
        expires_at_ms,
    })
}

fn decode_jwt_claims(token: &str) -> Result<Map<String, Value>, &'static str> {
    let mut parts = token.trim().split('.');
    let _header = parts.next().ok_or("invalid jwt")?;
    let payload_b64 = parts.next().ok_or("invalid jwt")?;
    let _sig = parts.next().ok_or("invalid jwt")?;
    if parts.next().is_some() {
        return Err("invalid jwt");
    }
    let raw = base64_decode(payload_b64)?;
    let Value::Object(map) = serde_json::from_slice::<Value>(&raw).map_err(|_| "invalid json")?
    else {
        return Err("invalid claims");
    };
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waits_for_a_fresh_totp_window_near_the_edge() {
        let base = 1_800_000_010_000_i64 - 1_800_000_010_000_i64.rem_euclid(30_000);
        assert_eq!(totp_wait_ms(base), 0);
        assert_eq!(totp_wait_ms(base + 25_999), 0);
        assert_eq!(totp_wait_ms(base + 26_000), 4_300);
        assert_eq!(totp_wait_ms(base + 29_999), 301);
    }

    #[test]
    fn ambiguous_workspace_error_lists_choices() {
        let candidates = vec![
            WorkspaceCandidate {
                id: "ws-a".into(),
                name: "个人".into(),
                project_id: String::new(),
            },
            WorkspaceCandidate {
                id: "ws-b".into(),
                name: "Team".into(),
                project_id: String::new(),
            },
        ];
        let e = LoginError::new("authorize", "workspace_ambiguous", "x")
            .with_choices("workspace", &candidates);
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(
            v["choices"][1],
            json!({"kind": "workspace", "id": "ws-b", "name": "Team"})
        );
        let plain = serde_json::to_value(LoginError::new("a", "b", "c")).unwrap();
        assert!(plain.get("choices").is_none());
    }

    fn make_jwt(payload: &Value) -> String {
        let header = base64_url_encode(br#"{"alg":"none","typ":"JWT"}"#);
        let body = base64_url_encode(&serde_json::to_vec(payload).unwrap());
        format!("{header}.{body}.sig")
    }

    #[test]
    fn parses_jwt_identity_and_callback() {
        let jwt = make_jwt(&json!({
            "sub": "auth0|user123",
            "email": "User@Example.com",
            "exp": 4_102_444_800_i64,
            "https://api.openai.com/auth": {
                "chatgpt_account_id": "ws-123",
                "chatgpt_user_id": "user-123",
                "chatgpt_plan_type": "Team",
                "poid": "org-123"
            }
        }));
        let id = parse_token_identity(&jwt, "", 1_700_000_000_000).unwrap();
        assert_eq!(id.account_id, "ws-123");
        assert_eq!(id.user_id, "user-123");
        assert_eq!(id.email, "user@example.com");
        assert_eq!(id.plan_type, "team");
        assert_eq!(id.subject, "auth0|user123");
        assert_eq!(id.poid.as_deref(), Some("org-123"));

        let (cb, code) = parse_callback_url(
            "http://localhost:1455/auth/callback?code=abc_123&state=st_1",
            DEFAULT_REDIRECT,
            "st_1",
        )
        .unwrap();
        assert_eq!(code, "abc_123");
        assert!(cb.starts_with("http://localhost:1455/auth/callback"));
    }

    #[test]
    fn decodes_session_cookie_with_prefix() {
        let session_json = json!({
            "client_auth_session": {
                "workspace_id": "ws_abc",
                "workspaces": [{"id": "ws_abc", "name": "Personal"}]
            }
        });
        let encoded = format!(
            "s:{}.sig",
            base64_url_encode(&serde_json::to_vec(&session_json).unwrap())
        );
        let parsed = decode_session_cookie(&encoded).unwrap();
        assert_eq!(string_field(&parsed, "workspace_id"), "ws_abc");
    }

    #[test]
    fn authorize_params_unwraps_the_cpr_desktop_auth_link() {
        let inner = "https://auth.openai.com/oauth/authorize?response_type=code&redirect_uri=http%3A%2F%2F127.0.0.1%3A1455%2Fauth%2Fcallback&state=s1";
        let outer = format!(
            "https://chatgpt.com/codex/desktop-auth?authorize_url={}&codex_streamlined_login=true",
            inner
                .replace('%', "%25")
                .replace(':', "%3A")
                .replace('/', "%2F")
                .replace('?', "%3F")
                .replace('&', "%26")
                .replace('=', "%3D")
        );
        let want = AuthorizeParams {
            redirect_uri: Some("http://127.0.0.1:1455/auth/callback".into()),
            state: "s1".into(),
        };
        assert_eq!(authorize_params(&outer), want);
        assert_eq!(authorize_params(inner), want);
        assert_eq!(
            authorize_params("https://chatgpt.com/x"),
            AuthorizeParams::default()
        );
    }
}

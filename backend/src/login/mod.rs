//! 登录：用登录凭据（邮箱、密码、2FA）换令牌并写回 CPR，以及快速导入新账号
//!
//! - [`routes`]：本模块的管理路由，路径都以 `api/login/` 开头，由 `management` 合进注册
//! - [`handle`]：`api/login/` 开头的请求交给它，返回 JSON 或 [`ApiError`]
//! - [`Login`]：进程内状态，插件启动时用 [`Config`] 建一次

pub mod crypto;
pub mod oauth;
pub mod paste;
pub mod sentinel;
pub mod totp;

use std::collections::HashMap;
use std::sync::Mutex;

use gateway_plugin_sdk::{
    ErrorCode, PluginFault,
    call::{
        host::{AuthRuntimeAccount, AuthSaveRequest, AuthSaveResult, CredentialFacts, LogLevel},
        management::{ManagementRequest, ManagementRoute},
    },
    client::{HostClient, TypedCall},
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::api::{self, ApiError};
use crate::chatgpt::supported;
use crate::clock::now_ms;
use crate::config::Config;
use crate::host;

use self::crypto::{CryptoError, Keyring, credential_aad};
use self::oauth::{
    Carrier, LoginError, LoginInput, Tokens, authorize_url, exchange_authorization_code,
    login_and_exchange, new_pkce, normalize_redirect_url, parse_callback_url,
};
use self::paste::{account_kind, is_valid_email, normalize_kind, parse_paste_lines};
use self::sentinel::{Identity, ensure_identity, is_uuid_v4};
use self::totp::validate_totp_secret;

/// 本模块路由的前缀
pub const PREFIX: &str = "api/login/";
/// 私有状态命名空间（在 `plugin.json` 的 `state` 中声明）
pub const STATE_NAMESPACE: &str = "login";
const OAUTH_SESSION_TTL_MS: i64 = 15 * 60 * 1000;

/// 登录模块的进程内状态
pub struct Login {
    keyring: Keyring,
    redirect_url: String,
    pending_oauth: Mutex<HashMap<String, PendingOAuth>>,
}

#[derive(Debug, Clone)]
struct PendingOAuth {
    state: String,
    verifier: String,
    authorize_url: String,
    email: String,
    kind: String,
    via_account_id: String,
    expires_at_ms: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct StoredLoginRecord {
    pub email: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password_enc: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub totp_enc: Option<String>,
    pub identity: Identity,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_login_at_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<StoredLoginError>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredLoginError {
    pub stage: String,
    pub code: String,
    pub message: String,
    pub at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountLoginStatus {
    pub account_id: String,
    pub name: String,
    pub email: String,
    pub kind: String,
    pub enabled: bool,
    pub credential_state: String,
    pub has_password: bool,
    pub has_totp: bool,
    pub identity: Identity,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_login_at_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<StoredLoginError>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_oauth: Option<PendingOAuthView>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingOAuthView {
    pub authorize_url: String,
    pub expires_at_ms: i64,
}

impl Login {
    #[must_use]
    pub fn new(config: &Config) -> Self {
        Self {
            keyring: Keyring::from_config(config.raw_encryption_keys(), &config.active_key),
            redirect_url: normalize_redirect_url(&config.oauth_redirect),
            pending_oauth: Mutex::new(HashMap::new()),
        }
    }

    fn require_keyring(&self) -> Result<&Keyring, ApiError> {
        if let Some(err) = self.keyring.config_error() {
            return Err(ApiError::new(400, "encryption_key_invalid", err));
        }
        if !self.keyring.is_configured() {
            return Err(ApiError::new(
                400,
                "encryption_key_missing",
                "尚未配置凭据加密密钥，请先在插件设置中填写 encryptionKey",
            ));
        }
        Ok(&self.keyring)
    }

    fn get_pending_oauth(&self, key: &str, now: i64) -> Option<PendingOAuth> {
        let mut map = self
            .pending_oauth
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        map.retain(|_, v| v.expires_at_ms > now);
        map.get(key).cloned()
    }

    fn set_pending_oauth(&self, key: String, pending: PendingOAuth, now: i64) {
        let mut map = self
            .pending_oauth
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        map.retain(|_, v| v.expires_at_ms > now);
        map.insert(key, pending);
    }

    fn take_pending_oauth(&self, key: &str, now: i64) -> Option<PendingOAuth> {
        let mut map = self
            .pending_oauth
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        map.retain(|_, v| v.expires_at_ms > now);
        map.remove(key)
    }
}

/// 本模块的管理路由（`api::route("POST", "api/login/…")`）
#[must_use]
pub fn routes() -> Vec<ManagementRoute> {
    vec![
        api::route("GET", "api/login/status"),
        api::route("POST", "api/login/credentials"),
        api::route("POST", "api/login/run"),
        api::route("POST", "api/login/oauth/start"),
        api::route("POST", "api/login/oauth/complete"),
        api::route("POST", "api/login/paste"),
        api::route("POST", "api/login/import"),
    ]
}

/// 处理 `api/login/` 开头的请求
///
/// # Errors
/// 请求无效或处理失败时返回错误
pub async fn handle(login: &Login, call: &TypedCall<ManagementRequest>) -> Result<Value, ApiError> {
    let host = &call.host;
    let req = &call.request;
    let query = api::parse_query(&req.query);
    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "api/login/status") => handle_status(login, host, &query).await,
        ("POST", "api/login/credentials") => {
            let body: CredentialsRequest = api::decode(call)?;
            handle_save_credentials(login, host, body).await
        }
        ("POST", "api/login/run") => {
            let body: RunLoginRequest = api::decode(call)?;
            handle_run_login(login, host, body).await
        }
        ("POST", "api/login/oauth/start") => {
            let body: OAuthStartRequest = api::decode(call)?;
            handle_oauth_start(login, host, body).await
        }
        ("POST", "api/login/oauth/complete") => {
            let body: OAuthCompleteRequest = api::decode(call)?;
            handle_oauth_complete(login, host, body).await
        }
        ("POST", "api/login/paste") => {
            let body: PasteRequest = api::decode(call)?;
            handle_paste(login, host, body).await
        }
        ("POST", "api/login/import") => {
            let body: ImportRequest = api::decode(call)?;
            handle_import(login, host, body).await
        }
        _ => Err(ApiError::new(404, "not_found", "没有这个接口")),
    }
}

async fn handle_status(
    login: &Login,
    host: &HostClient,
    query: &HashMap<String, String>,
) -> Result<Value, ApiError> {
    let now = now_ms();
    if let Some(id) = query.get("id").map(|s| s.trim()).filter(|s| !s.is_empty()) {
        let acct = host::account(host, id).await?;
        if !supported(&acct) {
            return Err(ApiError::new(
                409,
                "unsupported_account",
                "只支持 OpenAI OAuth 账号",
            ));
        }
        let (rec, _) = load_record(host, id).await?;
        let status = build_account_status(login, &acct, &rec, now);
        return Ok(json!({
            "encryptionConfigured": login.keyring.is_configured(),
            "encryptionError": login.keyring.config_error(),
            "oauthRedirect": login.redirect_url,
            "account": status,
        }));
    }

    let cpr_accounts = host::openai_accounts(host).await?;
    let mut accounts = Vec::new();
    for acct in cpr_accounts.into_iter().filter(supported) {
        let (rec, _) = load_record(host, &acct.account_id).await?;
        accounts.push(build_account_status(login, &acct, &rec, now));
    }
    Ok(json!({
        "encryptionConfigured": login.keyring.is_configured(),
        "encryptionError": login.keyring.config_error(),
        "oauthRedirect": login.redirect_url,
        "accounts": accounts,
    }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", default)]
#[derive(Default)]
struct CredentialsRequest {
    account_id: String,
    email: Option<String>,
    password: Option<String>,
    totp: Option<String>,
    clear_password: bool,
    clear_totp: bool,
}

async fn handle_save_credentials(
    login: &Login,
    host: &HostClient,
    req: CredentialsRequest,
) -> Result<Value, ApiError> {
    let account_id = req.account_id.trim();
    if account_id.is_empty() {
        return Err(ApiError::invalid("缺少参数 accountId"));
    }
    let acct = host::account(host, account_id).await?;
    if !supported(&acct) {
        return Err(ApiError::new(
            409,
            "unsupported_account",
            "只支持 OpenAI OAuth 账号",
        ));
    }

    let mut rec = load_record(host, account_id).await?.0;
    let new_pw = req.password.as_deref().map(str::trim).unwrap_or_default();
    let new_totp = req.totp.as_deref().map(str::trim).unwrap_or_default();

    if (!new_pw.is_empty() || !new_totp.is_empty()) && !login.keyring.is_configured() {
        let _ = login.require_keyring()?;
    }

    if req.clear_password {
        rec.password_enc = None;
    } else if !new_pw.is_empty() {
        let kr = login.require_keyring()?;
        let aad = credential_aad(account_id, "password");
        rec.password_enc = Some(kr.seal(new_pw, &aad).map_err(crypto_api_error)?);
    }

    if req.clear_totp {
        rec.totp_enc = None;
    } else if !new_totp.is_empty() {
        let normalized = validate_totp_secret(new_totp).map_err(|code| {
            ApiError::new(
                400,
                code,
                "2FA 密钥格式无效（需为 Base32 密钥或 6位/30秒/SHA1 的 otpauth://totp/ 链接）",
            )
        })?;
        let kr = login.require_keyring()?;
        let aad = credential_aad(account_id, "totp");
        rec.totp_enc = Some(kr.seal(&normalized, &aad).map_err(crypto_api_error)?);
    }

    if let Some(email) = req
        .email
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if !is_valid_email(email) {
            return Err(ApiError::new(400, "invalid_email", "邮箱地址格式无效"));
        }
        rec.email = email.to_ascii_lowercase();
    } else if rec.email.is_empty()
        && let Some(email) = acct.email.as_deref()
    {
        rec.email = email.trim().to_ascii_lowercase();
    }
    if rec.kind.is_empty() {
        rec.kind = account_kind(acct.plan_type.as_deref()).to_owned();
    }
    let (identity, _, _) = ensure_identity(&rec.identity);
    rec.identity = identity;

    save_record(host, account_id, &rec).await?;
    let status = build_account_status(login, &acct, &rec, now_ms());
    Ok(json!({
        "ok": true,
        "account": status,
    }))
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct RunLoginRequest {
    account_id: String,
    email: String,
    password: String,
    totp: String,
    save_credentials: bool,
    workspace_id: String,
    organization_id: String,
}

async fn handle_run_login(
    login: &Login,
    host: &HostClient,
    req: RunLoginRequest,
) -> Result<Value, ApiError> {
    let account_id = req.account_id.trim();
    if account_id.is_empty() {
        return Err(ApiError::invalid("缺少参数 accountId"));
    }
    let acct = host::account(host, account_id).await?;
    if !supported(&acct) {
        return Err(ApiError::new(
            409,
            "unsupported_account",
            "只支持 OpenAI OAuth 账号",
        ));
    }
    if !acct.enabled {
        return Err(ApiError::new(
            409,
            "account_unavailable",
            "账号在 CPR 里已停用，停用的账号不能经它的代理发请求",
        ));
    }

    let mut rec = load_record(host, account_id).await?.0;
    let email = if !req.email.trim().is_empty() {
        req.email.trim().to_ascii_lowercase()
    } else if let Some(e) = acct
        .email
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        e.to_ascii_lowercase()
    } else {
        rec.email.trim().to_ascii_lowercase()
    };
    if email.is_empty() {
        return Err(ApiError::new(400, "email_missing", "账号缺少登录邮箱"));
    }
    if !is_valid_email(&email) {
        return Err(ApiError::new(400, "invalid_email", "邮箱地址格式无效"));
    }

    let password = if !req.password.is_empty() {
        req.password.clone()
    } else if let Some(ref enc) = rec.password_enc {
        let kr = login.require_keyring()?;
        let aad = credential_aad(account_id, "password");
        kr.open(enc, &aad).map_err(crypto_api_error)?
    } else {
        return Err(ApiError::new(
            400,
            "password_missing",
            "未提供登录密码，且该账号尚未保存加密密码",
        ));
    };

    let totp_secret = if !req.totp.trim().is_empty() {
        validate_totp_secret(&req.totp).map_err(|code| {
            ApiError::new(
                400,
                code,
                "2FA 密钥格式无效（需为 Base32 密钥或 6位/30秒/SHA1 的 otpauth://totp/ 链接）",
            )
        })?
    } else if let Some(ref enc) = rec.totp_enc {
        let kr = login.require_keyring()?;
        let aad = credential_aad(account_id, "totp");
        kr.open(enc, &aad).map_err(crypto_api_error)?
    } else {
        String::new()
    };

    if req.save_credentials && (!req.password.is_empty() || !req.totp.trim().is_empty()) {
        let kr = login.require_keyring()?;
        if !req.password.is_empty() {
            let aad = credential_aad(account_id, "password");
            rec.password_enc = Some(kr.seal(&password, &aad).map_err(crypto_api_error)?);
        }
        if !req.totp.trim().is_empty() {
            let aad = credential_aad(account_id, "totp");
            rec.totp_enc = Some(kr.seal(&totp_secret, &aad).map_err(crypto_api_error)?);
        }
    }

    rec.email.clone_from(&email);
    if rec.kind.is_empty() {
        rec.kind = account_kind(acct.plan_type.as_deref()).to_owned();
    }
    let (identity, _, _) = ensure_identity(&rec.identity);
    rec.identity = identity.clone();

    let carrier = Carrier {
        account_id: acct.account_id.clone(),
        credential_revision: acct.credential_revision,
    };
    let input = LoginInput {
        email: email.clone(),
        password,
        totp_secret,
        workspace_id: req.workspace_id,
        organization_id: req.organization_id,
        expected_workspace_id: acct.upstream_account_id.clone(),
        identity,
        redirect_url: login.redirect_url.clone(),
    };

    match login_and_exchange(host, &carrier, &input).await {
        Err(err) => {
            let now = now_ms();
            rec.last_error = Some(StoredLoginError {
                stage: err.stage.to_owned(),
                code: err.code.to_owned(),
                message: err.message.clone(),
                at_ms: now,
            });
            let _ = save_record(host, account_id, &rec).await;
            host::log(
                host,
                "login_failed",
                LogLevel::Warn,
                json!({
                    "account_id": account_id,
                    "stage": err.stage,
                    "code": err.code,
                }),
            )
            .await;
            Err(login_error_to_api(&err))
        }
        Ok(out) => {
            rec.identity = out.identity;
            save_replaced_credential(host, &acct, &out.tokens, &rec.identity).await?;
            let now = now_ms();
            rec.last_login_at_ms = Some(now);
            rec.last_error = None;
            if !out.tokens.identity.plan_type.is_empty() {
                rec.kind = account_kind(Some(&out.tokens.identity.plan_type)).to_owned();
            }
            save_record(host, account_id, &rec).await?;
            let _ = login.take_pending_oauth(account_id, now);
            host::log(
                host,
                "login_succeeded",
                LogLevel::Info,
                json!({
                    "account_id": account_id,
                    "plan_type": out.tokens.identity.plan_type,
                }),
            )
            .await;
            let refreshed_acct = host::account(host, account_id).await.unwrap_or(acct);
            let status = build_account_status(login, &refreshed_acct, &rec, now);
            Ok(json!({
                "ok": true,
                "account": status,
                "planType": out.tokens.identity.plan_type,
                "expiresAtMs": out.tokens.identity.expires_at_ms,
            }))
        }
    }
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct OAuthStartRequest {
    account_id: String,
    email: String,
    kind: String,
    via_account_id: String,
}

async fn handle_oauth_start(
    login: &Login,
    host: &HostClient,
    req: OAuthStartRequest,
) -> Result<Value, ApiError> {
    let now = now_ms();
    let (session_key, email, kind, via_account_id) = if !req.account_id.trim().is_empty() {
        let account_id = req.account_id.trim();
        let acct = host::account(host, account_id).await?;
        if !supported(&acct) {
            return Err(ApiError::new(
                409,
                "unsupported_account",
                "只支持 OpenAI OAuth 账号",
            ));
        }
        if !acct.enabled {
            return Err(ApiError::new(
                409,
                "account_unavailable",
                "账号在 CPR 里已停用，停用的账号不能经它的代理发请求",
            ));
        }
        let email = if !req.email.trim().is_empty() {
            req.email.trim().to_ascii_lowercase()
        } else {
            acct.email
                .as_deref()
                .unwrap_or_default()
                .trim()
                .to_ascii_lowercase()
        };
        let kind = account_kind(acct.plan_type.as_deref()).to_owned();
        (account_id.to_owned(), email, kind, account_id.to_owned())
    } else {
        let email = req.email.trim().to_ascii_lowercase();
        if !is_valid_email(&email) {
            return Err(ApiError::new(400, "invalid_email", "邮箱地址格式无效"));
        }
        let kind = normalize_kind(&req.kind)
            .map_err(|code| ApiError::new(400, code, "账号类型无效"))?
            .to_owned();
        let carrier = resolve_carrier(host, &req.via_account_id).await?;
        (
            import_session_key(&email, &kind),
            email,
            kind,
            carrier.account_id,
        )
    };

    let (state, verifier, challenge) = new_pkce().map_err(|e| login_error_to_api(&e))?;
    let auth_url = authorize_url(&login.redirect_url, &state, &challenge);
    let expires_at_ms = now + OAUTH_SESSION_TTL_MS;

    login.set_pending_oauth(
        session_key,
        PendingOAuth {
            state,
            verifier,
            authorize_url: auth_url.clone(),
            email,
            kind,
            via_account_id,
            expires_at_ms,
        },
        now,
    );

    Ok(json!({
        "authorizeUrl": auth_url,
        "redirectUrl": login.redirect_url,
        "expiresAtMs": expires_at_ms,
    }))
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct OAuthCompleteRequest {
    account_id: String,
    email: String,
    kind: String,
    via_account_id: String,
    callback_url: String,
}

async fn handle_oauth_complete(
    login: &Login,
    host: &HostClient,
    req: OAuthCompleteRequest,
) -> Result<Value, ApiError> {
    let now = now_ms();
    let is_existing = !req.account_id.trim().is_empty();
    let session_key = if is_existing {
        req.account_id.trim().to_owned()
    } else {
        let email = req.email.trim().to_ascii_lowercase();
        let kind =
            normalize_kind(&req.kind).map_err(|code| ApiError::new(400, code, "账号类型无效"))?;
        import_session_key(&email, kind)
    };

    let Some(pending) = login.get_pending_oauth(&session_key, now) else {
        return Err(ApiError::new(
            400,
            "oauth_session_expired",
            "未找到进行中的手动 OAuth 授权会话或会话已过期，请重新生成授权链接",
        ));
    };

    let (_, code) = parse_callback_url(&req.callback_url, &login.redirect_url, &pending.state)
        .map_err(|e| login_error_to_api(&e))?;

    if is_existing {
        let account_id = req.account_id.trim();
        let acct = host::account(host, account_id).await?;
        if !supported(&acct) {
            return Err(ApiError::new(
                409,
                "unsupported_account",
                "只支持 OpenAI OAuth 账号",
            ));
        }
        if !acct.enabled {
            return Err(ApiError::new(
                409,
                "account_unavailable",
                "账号在 CPR 里已停用，停用的账号不能经它的代理发请求",
            ));
        }
        let mut rec = load_record(host, account_id).await?.0;
        let (identity, profile, _) = ensure_identity(&rec.identity);
        rec.identity = identity;
        let carrier = Carrier {
            account_id: acct.account_id.clone(),
            credential_revision: acct.credential_revision,
        };
        let expected_email = if !pending.email.is_empty() {
            pending.email.as_str()
        } else {
            acct.email.as_deref().unwrap_or_default()
        };
        let tokens = exchange_authorization_code(
            host,
            &carrier,
            &login.redirect_url,
            &code,
            &pending.verifier,
            expected_email,
            &profile,
        )
        .await
        .map_err(|e| login_error_to_api(&e))?;

        save_replaced_credential(host, &acct, &tokens, &rec.identity).await?;
        rec.last_login_at_ms = Some(now);
        rec.last_error = None;
        if rec.email.is_empty() && !tokens.identity.email.is_empty() {
            rec.email.clone_from(&tokens.identity.email);
        }
        save_record(host, account_id, &rec).await?;
        let _ = login.take_pending_oauth(&session_key, now);
        let refreshed = host::account(host, account_id).await.unwrap_or(acct);
        let status = build_account_status(login, &refreshed, &rec, now);
        return Ok(json!({
            "ok": true,
            "accountId": account_id,
            "account": status,
        }));
    }

    let via_id = if !req.via_account_id.trim().is_empty() {
        req.via_account_id.trim()
    } else {
        pending.via_account_id.as_str()
    };
    let carrier = resolve_carrier(host, via_id).await?;
    let staged_key = staged_email_key(&pending.email, &pending.kind);
    let mut staged_rec = load_record(host, &staged_key).await?.0;
    let (identity, profile, _) = ensure_identity(&staged_rec.identity);
    staged_rec.identity = identity.clone();

    let tokens = exchange_authorization_code(
        host,
        &carrier,
        &login.redirect_url,
        &code,
        &pending.verifier,
        &pending.email,
        &profile,
    )
    .await
    .map_err(|e| login_error_to_api(&e))?;

    let saved =
        save_created_credential(host, &pending.email, &pending.kind, &tokens, &identity).await?;

    let mut new_rec = StoredLoginRecord {
        email: pending.email.clone(),
        kind: pending.kind.clone(),
        password_enc: None,
        totp_enc: None,
        identity,
        last_login_at_ms: Some(now),
        last_error: None,
    };
    if login.keyring.is_configured() {
        if let Some(ref enc_pw) = staged_rec.password_enc
            && let Ok(plain_pw) = login
                .keyring
                .open(enc_pw, &credential_aad(&staged_key, "password"))
        {
            new_rec.password_enc = login
                .keyring
                .seal(&plain_pw, &credential_aad(&saved.account_id, "password"))
                .ok();
        }
        if let Some(ref enc_totp) = staged_rec.totp_enc
            && let Ok(plain_totp) = login
                .keyring
                .open(enc_totp, &credential_aad(&staged_key, "totp"))
        {
            new_rec.totp_enc = login
                .keyring
                .seal(&plain_totp, &credential_aad(&saved.account_id, "totp"))
                .ok();
        }
    }
    save_record(host, &saved.account_id, &new_rec).await?;
    let _ = login.take_pending_oauth(&session_key, now);

    Ok(json!({
        "ok": true,
        "accountId": saved.account_id,
        "email": pending.email,
        "kind": pending.kind,
    }))
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct PasteRequest {
    text: String,
    apply: bool,
}

async fn handle_paste(
    login: &Login,
    host: &HostClient,
    req: PasteRequest,
) -> Result<Value, ApiError> {
    let cpr_accounts = host::openai_accounts(host).await?;
    let (mut report, rows) = parse_paste_lines(&req.text, &cpr_accounts);
    if !req.apply {
        return serde_json::to_value(report).map_err(|_| ApiError::invalid("序列化预览结果失败"));
    }

    let kr = login.require_keyring()?;
    for row in rows {
        let target_key = if let Some(ref existing_id) = row.item.existing_account_id {
            existing_id.clone()
        } else {
            staged_email_key(&row.item.email, &row.item.kind)
        };
        let mut rec = load_record(host, &target_key).await?.0;
        rec.email.clone_from(&row.item.email);
        rec.kind.clone_from(&row.item.kind);
        let (identity, _, _) = ensure_identity(&rec.identity);
        rec.identity = identity;
        rec.password_enc = Some(
            kr.seal(&row.password, &credential_aad(&target_key, "password"))
                .map_err(crypto_api_error)?,
        );
        if !row.totp_secret.is_empty() {
            rec.totp_enc = Some(
                kr.seal(&row.totp_secret, &credential_aad(&target_key, "totp"))
                    .map_err(crypto_api_error)?,
            );
        } else {
            rec.totp_enc = None;
        }
        save_record(host, &target_key, &rec).await?;
    }
    report.applied = true;
    serde_json::to_value(report).map_err(|_| ApiError::invalid("序列化导入结果失败"))
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct ImportRequest {
    email: String,
    password: String,
    totp: String,
    kind: String,
    via_account_id: String,
    workspace_id: String,
    organization_id: String,
    replace_existing: bool,
    save_credentials: bool,
}

async fn handle_import(
    login: &Login,
    host: &HostClient,
    req: ImportRequest,
) -> Result<Value, ApiError> {
    let email = req.email.trim().to_ascii_lowercase();
    if email.is_empty() {
        return Err(ApiError::new(400, "email_missing", "缺少登录邮箱"));
    }
    if !is_valid_email(&email) {
        return Err(ApiError::new(400, "invalid_email", "邮箱地址格式无效"));
    }
    let kind = normalize_kind(&req.kind)
        .map_err(|code| ApiError::new(400, code, "账号类型必须是 team、personal 或 个人"))?
        .to_owned();

    let cpr_accounts = host::openai_accounts(host).await?;
    let existing = cpr_accounts.iter().find(|a| {
        supported(a)
            && a.email
                .as_deref()
                .is_some_and(|e| e.trim().eq_ignore_ascii_case(&email))
            && account_kind(a.plan_type.as_deref()) == kind
    });

    if let Some(existing_acct) = existing {
        if !req.replace_existing {
            return Err(ApiError::new(
                409,
                "already_in_source",
                "该账号已存在于 CPR 中",
            ));
        }
        return handle_run_login(
            login,
            host,
            RunLoginRequest {
                account_id: existing_acct.account_id.clone(),
                email,
                password: req.password,
                totp: req.totp,
                save_credentials: req.save_credentials || login.keyring.is_configured(),
                workspace_id: req.workspace_id,
                organization_id: req.organization_id,
            },
        )
        .await;
    }

    let staged_key = staged_email_key(&email, &kind);
    let mut staged_rec = load_record(host, &staged_key).await?.0;

    let password = if !req.password.is_empty() {
        req.password.clone()
    } else if let Some(ref enc) = staged_rec.password_enc {
        let kr = login.require_keyring()?;
        kr.open(enc, &credential_aad(&staged_key, "password"))
            .map_err(crypto_api_error)?
    } else {
        return Err(ApiError::new(400, "password_missing", "缺少登录密码"));
    };

    let totp_secret = if !req.totp.trim().is_empty() {
        validate_totp_secret(&req.totp).map_err(|code| {
            ApiError::new(
                400,
                code,
                "2FA 密钥格式无效（需为 Base32 密钥或 6位/30秒/SHA1 的 otpauth://totp/ 链接）",
            )
        })?
    } else if let Some(ref enc) = staged_rec.totp_enc {
        let kr = login.require_keyring()?;
        kr.open(enc, &credential_aad(&staged_key, "totp"))
            .map_err(crypto_api_error)?
    } else {
        String::new()
    };

    let carrier = resolve_carrier(host, &req.via_account_id).await?;
    let (identity, _, _) = ensure_identity(&staged_rec.identity);
    staged_rec.identity = identity.clone();

    let input = LoginInput {
        email: email.clone(),
        password: password.clone(),
        totp_secret: totp_secret.clone(),
        workspace_id: req.workspace_id,
        organization_id: req.organization_id,
        expected_workspace_id: None,
        identity,
        redirect_url: login.redirect_url.clone(),
    };

    let out = match login_and_exchange(host, &carrier, &input).await {
        Ok(out) => out,
        Err(err) => {
            host::log(
                host,
                "import_login_failed",
                LogLevel::Warn,
                json!({
                    "email": email,
                    "kind": kind,
                    "stage": err.stage,
                    "code": err.code,
                }),
            )
            .await;
            return Err(login_error_to_api(&err));
        }
    };

    let saved = save_created_credential(host, &email, &kind, &out.tokens, &out.identity).await?;
    let now = now_ms();
    let actual_kind = if !out.tokens.identity.plan_type.is_empty() {
        account_kind(Some(&out.tokens.identity.plan_type)).to_owned()
    } else {
        kind.clone()
    };

    let mut new_rec = StoredLoginRecord {
        email: email.clone(),
        kind: actual_kind.clone(),
        password_enc: None,
        totp_enc: None,
        identity: out.identity,
        last_login_at_ms: Some(now),
        last_error: None,
    };

    if login.keyring.is_configured() {
        let pw_aad = credential_aad(&saved.account_id, "password");
        new_rec.password_enc = Some(
            login
                .keyring
                .seal(&password, &pw_aad)
                .map_err(crypto_api_error)?,
        );
        if !totp_secret.is_empty() {
            let totp_aad = credential_aad(&saved.account_id, "totp");
            new_rec.totp_enc = Some(
                login
                    .keyring
                    .seal(&totp_secret, &totp_aad)
                    .map_err(crypto_api_error)?,
            );
        }
    }

    save_record(host, &saved.account_id, &new_rec).await?;
    host::log(
        host,
        "import_succeeded",
        LogLevel::Info,
        json!({
            "account_id": saved.account_id,
            "kind": actual_kind,
        }),
    )
    .await;

    Ok(json!({
        "ok": true,
        "accountId": saved.account_id,
        "email": email,
        "kind": actual_kind,
        "planType": out.tokens.identity.plan_type,
        "hasPassword": new_rec.password_enc.is_some(),
        "hasTotp": new_rec.totp_enc.is_some(),
    }))
}

async fn resolve_carrier(host: &HostClient, via_account_id: &str) -> Result<Carrier, ApiError> {
    let trimmed = via_account_id.trim();
    if !trimmed.is_empty() {
        let acct = host::account(host, trimmed).await?;
        if !supported(&acct) {
            return Err(ApiError::new(
                409,
                "unsupported_account",
                "指定的代理承载账号不是 OpenAI OAuth 账号",
            ));
        }
        if !acct.enabled {
            return Err(ApiError::new(
                409,
                "source_proxy_unavailable",
                "指定的代理承载账号在 CPR 中已停用，无法经它发起登录请求",
            ));
        }
        return Ok(Carrier {
            account_id: acct.account_id,
            credential_revision: acct.credential_revision,
        });
    }
    let all = host::openai_accounts(host).await?;
    let Some(acct) = all.into_iter().find(|a| supported(a) && a.enabled) else {
        return Err(ApiError::new(
            409,
            "source_proxy_unavailable",
            "CPR 中没有处于启用状态的 OpenAI 账号可用于代理出站请求",
        ));
    };
    Ok(Carrier {
        account_id: acct.account_id,
        credential_revision: acct.credential_revision,
    })
}

async fn save_replaced_credential(
    host: &HostClient,
    acct: &AuthRuntimeAccount,
    tokens: &Tokens,
    identity: &Identity,
) -> Result<AuthSaveResult, ApiError> {
    for attempt in 0..2 {
        let current = host::credential(host, &acct.account_id).await?;
        let mut material = current.facts.material;
        material.insert("schema_version".into(), json!(1));
        let has_valid_install_id = material
            .get("installation_id")
            .and_then(Value::as_str)
            .is_some_and(is_uuid_v4);
        if !has_valid_install_id {
            material.insert("installation_id".into(), json!(identity.device_id));
        }
        material.insert("access_token".into(), json!(tokens.access_token));
        material.insert("refresh_token".into(), json!(tokens.refresh_token));
        material.insert("id_token".into(), json!(tokens.id_token));
        if !tokens.identity.subject.is_empty() || tokens.identity.poid.is_some() {
            material.insert(
                "principal".into(),
                json!({
                    "oauth_subject": tokens.identity.subject,
                    "poid": tokens.identity.poid,
                }),
            );
        }

        let name = if !current.facts.name.trim().is_empty() {
            current.facts.name
        } else if !acct.name.trim().is_empty() {
            acct.name.clone()
        } else {
            tokens.identity.email.clone()
        };
        let email = if !tokens.identity.email.is_empty() {
            Some(tokens.identity.email.clone())
        } else {
            current.facts.email.or_else(|| acct.email.clone())
        };
        let upstream_user_id = if !tokens.identity.user_id.is_empty() {
            Some(tokens.identity.user_id.clone())
        } else {
            current
                .facts
                .upstream_user_id
                .or_else(|| acct.upstream_user_id.clone())
        };
        let plan_type = if !tokens.identity.plan_type.is_empty() {
            Some(tokens.identity.plan_type.clone())
        } else {
            current.facts.plan_type.or_else(|| acct.plan_type.clone())
        };

        let facts = CredentialFacts {
            name: truncate_name(&name, "openai-account"),
            authentication_kind: "oauth".to_owned(),
            material,
            email,
            upstream_user_id,
            upstream_account_id: Some(tokens.identity.account_id.clone()),
            plan_type,
            has_refresh_token: !tokens.refresh_token.is_empty(),
            access_token_expires_at_ms: Some(tokens.identity.expires_at_ms),
            next_refresh_at_ms: None,
        };

        match host::auth_save(
            host,
            &AuthSaveRequest::Replace {
                account_id: acct.account_id.clone(),
                credential_revision: current.credential_revision,
                facts,
            },
        )
        .await
        {
            Ok(saved) => return Ok(saved),
            Err(e) if e.code == ErrorCode::Conflict && attempt == 0 => continue,
            Err(e) => return Err(ApiError::from(e)),
        }
    }
    Err(ApiError::new(
        409,
        "conflict",
        "账号凭据版本冲突，请稍后重试",
    ))
}

async fn save_created_credential(
    host: &HostClient,
    email: &str,
    kind: &str,
    tokens: &Tokens,
    identity: &Identity,
) -> Result<AuthSaveResult, ApiError> {
    let mut material = Map::new();
    material.insert("schema_version".into(), json!(1));
    material.insert("installation_id".into(), json!(identity.device_id));
    material.insert("access_token".into(), json!(tokens.access_token));
    material.insert("refresh_token".into(), json!(tokens.refresh_token));
    material.insert("id_token".into(), json!(tokens.id_token));
    material.insert("cookies".into(), json!([]));
    if !tokens.identity.subject.is_empty() || tokens.identity.poid.is_some() {
        material.insert(
            "principal".into(),
            json!({
                "oauth_subject": tokens.identity.subject,
                "poid": tokens.identity.poid,
            }),
        );
    }

    let final_email = if !tokens.identity.email.is_empty() {
        tokens.identity.email.clone()
    } else {
        email.to_owned()
    };
    let plan_type = if !tokens.identity.plan_type.is_empty() {
        tokens.identity.plan_type.clone()
    } else if kind == "team" {
        "team".to_owned()
    } else {
        "plus".to_owned()
    };

    let facts = CredentialFacts {
        name: truncate_name(&final_email, "openai-account"),
        authentication_kind: "oauth".to_owned(),
        material,
        email: Some(final_email),
        upstream_user_id: if tokens.identity.user_id.is_empty() {
            None
        } else {
            Some(tokens.identity.user_id.clone())
        },
        upstream_account_id: Some(tokens.identity.account_id.clone()),
        plan_type: Some(plan_type),
        has_refresh_token: !tokens.refresh_token.is_empty(),
        access_token_expires_at_ms: Some(tokens.identity.expires_at_ms),
        next_refresh_at_ms: None,
    };

    host::auth_save(
        host,
        &AuthSaveRequest::Create {
            provider_id: "openai".to_owned(),
            facts,
        },
    )
    .await
    .map_err(ApiError::from)
}

fn truncate_name(raw: &str, fallback: &str) -> String {
    let trimmed = raw.trim();
    let s = if trimmed.is_empty() {
        fallback
    } else {
        trimmed
    };
    s.chars().take(200).collect()
}

fn build_account_status(
    login: &Login,
    acct: &AuthRuntimeAccount,
    rec: &StoredLoginRecord,
    now: i64,
) -> AccountLoginStatus {
    let email = if let Some(ref e) = acct.email
        && !e.trim().is_empty()
    {
        e.trim().to_owned()
    } else {
        rec.email.clone()
    };
    let kind = account_kind(acct.plan_type.as_deref()).to_owned();
    let pending_oauth = login
        .get_pending_oauth(&acct.account_id, now)
        .map(|p| PendingOAuthView {
            authorize_url: p.authorize_url,
            expires_at_ms: p.expires_at_ms,
        });
    AccountLoginStatus {
        account_id: acct.account_id.clone(),
        name: acct.name.clone(),
        email,
        kind,
        enabled: acct.enabled,
        credential_state: acct.credential_state.clone(),
        has_password: rec.password_enc.is_some(),
        has_totp: rec.totp_enc.is_some(),
        identity: rec.identity.clone(),
        last_login_at_ms: rec.last_login_at_ms,
        last_error: rec.last_error.clone(),
        pending_oauth,
    }
}

async fn load_record(
    host: &HostClient,
    key: &str,
) -> Result<(StoredLoginRecord, Option<u64>), PluginFault> {
    let Some(record) = host::state_get(host, STATE_NAMESPACE, key).await? else {
        return Ok((StoredLoginRecord::default(), None));
    };
    let parsed = serde_json::from_value::<StoredLoginRecord>(record.value).unwrap_or_default();
    Ok((parsed, Some(record.version)))
}

async fn save_record(
    host: &HostClient,
    key: &str,
    rec: &StoredLoginRecord,
) -> Result<(), PluginFault> {
    let value = serde_json::to_value(rec)
        .map_err(|_| PluginFault::new(ErrorCode::Fault, "登录记录编码失败"))?;
    for attempt in 0..2 {
        let (_, ver) = load_record(host, key).await?;
        match host::state_put(host, STATE_NAMESPACE, key, value.clone(), ver).await {
            Ok(_) => return Ok(()),
            Err(e) if e.code == ErrorCode::Conflict && attempt == 0 => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

fn staged_email_key(email: &str, kind: &str) -> String {
    format!(
        "email:{}:{}",
        email.trim().to_ascii_lowercase(),
        kind.trim().to_ascii_lowercase()
    )
}

fn import_session_key(email: &str, kind: &str) -> String {
    format!(
        "import:{}:{}",
        email.trim().to_ascii_lowercase(),
        kind.trim().to_ascii_lowercase()
    )
}

fn crypto_api_error(err: CryptoError) -> ApiError {
    ApiError::new(400, err.code(), err.message())
}

fn login_error_to_api(err: &LoginError) -> ApiError {
    let status = match err.code {
        "email_missing"
        | "password_missing"
        | "totp_secret_missing"
        | "totp_invalid"
        | "invalid_callback"
        | "invalid_redirect" => 400,
        "password_rejected" | "totp_rejected" | "login_rejected" => 401,
        "login_forbidden" | "browser_challenge" => 403,
        "already_in_source"
        | "workspace_ambiguous"
        | "workspace_unavailable"
        | "email_otp_required"
        | "unsupported_challenge"
        | "unsupported_mfa"
        | "identity_mismatch"
        | "account_unavailable"
        | "credential_changed" => 409,
        "rate_limited" => 429,
        "network_timeout" => 504,
        _ => 502,
    };
    ApiError::new(status, err.code, err.message.clone())
}

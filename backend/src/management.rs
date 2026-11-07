//! 管理页面与接口（只有 CPR 的管理员能打开，页面在 CPR 的隔离 iframe 里）

use futures::{StreamExt, stream};
use gateway_plugin_sdk::{
    PluginFault,
    call::{
        host::AuthRuntimeAccount,
        management::{
            ManagementPage, ManagementRegistration, ManagementRequest, ManagementResource,
        },
    },
    client::{HostClient, TypedCall},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::App;
use crate::api::{self, ApiError, Reply, parse_query, required};
use crate::billing::{self, LastPayment};
use crate::chatgpt::{self, Problem, Session};
use crate::clock::now_ms;
use crate::devices;
use crate::host;
use crate::login;
use crate::store;
use crate::usage::{self, Summary};

/// 列表同时读几个账号的宿主事实
const LIST_CONCURRENCY: usize = 16;

pub fn registration() -> ManagementRegistration {
    let mut routes = vec![
        api::route("GET", "api/accounts"),
        api::route("GET", "api/usage"),
        api::route("GET", "api/subscription"),
        api::route("GET", "api/billing"),
        api::route("GET", "api/devices"),
        api::route("POST", "api/devices/sign-out"),
    ];
    routes.extend(login::routes());
    ManagementRegistration {
        routes,
        resources: ["web/index.html", "web/app.js", "web/app.css"]
            .into_iter()
            .map(|path| ManagementResource {
                path: path.to_owned(),
                public: false,
            })
            .collect(),
        pages: vec![ManagementPage {
            id: "chatgpt-accounts".to_owned(),
            title: "ChatGPT 账号".to_owned(),
            description: Some("官方用量（看 Work）、订阅与账单、登录设备".to_owned()),
            entry: "web/index.html".to_owned(),
            icon: None,
        }],
        callbacks: Vec::new(),
    }
}

pub async fn handle(app: &App, call: TypedCall<ManagementRequest>) -> Result<Reply, PluginFault> {
    match route(app, &call).await {
        Ok(body) => api::reply(200, &body),
        Err(e) => api::error_reply(&e),
    }
}

async fn route(app: &App, call: &TypedCall<ManagementRequest>) -> Result<Value, ApiError> {
    if call.payload.len() > api::MAX_BODY {
        return Err(ApiError::invalid("请求正文太大"));
    }
    if call.request.path.starts_with(login::PREFIX) {
        return login::handle(&app.login, call).await;
    }
    let host = &call.host;
    let query = parse_query(&call.request.query);
    match (call.request.method.as_str(), call.request.path.as_str()) {
        ("GET", "api/accounts") => accounts(host).await,
        ("GET", "api/usage") => {
            let days = query
                .get("days")
                .map_or(Ok(7), |v| v.parse::<u32>())
                .map_err(|_| ApiError::invalid("时间范围无效"))?;
            read_usage(host, required(&query, "id")?, days).await
        }
        ("GET", "api/subscription") => {
            let session = Session::open(host, required(&query, "id")?).await?;
            Ok(json!({"subscription": billing::subscription(&session).await?}))
        }
        ("GET", "api/billing") => {
            let id = required(&query, "id")?;
            let cursor = query.get("cursor").map_or("", String::as_str);
            read_billing(host, id, cursor).await
        }
        ("GET", "api/devices") => {
            let session = Session::open(host, required(&query, "id")?).await?;
            Ok(json!({"devices": devices::devices(&session).await?}))
        }
        ("POST", "api/devices/sign-out") => sign_out(host, api::decode(call)?).await,
        _ => Err(ApiError::new(404, "not_found", "没有这个接口")),
    }
}

/// 登出请求：`device`（一台）、`others: true`（CPR 会话以外的全部）或 `all: true`（全部，
/// 连 CPR 的会话）三选一
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SignOut {
    id: String,
    #[serde(default)]
    device: Option<String>,
    #[serde(default)]
    others: bool,
    #[serde(default)]
    all: bool,
}

async fn sign_out(host: &HostClient, req: SignOut) -> Result<Value, ApiError> {
    let chosen = usize::from(req.device.is_some()) + usize::from(req.others) + usize::from(req.all);
    if chosen != 1 {
        return Err(ApiError::invalid("device、others、all 只能选一个"));
    }
    let session = Session::open(host, &req.id).await?;
    let partial = |done: usize, p: Problem| {
        let mut e = ApiError::from(p);
        if done > 0 {
            e.message = format!("已登出 {done} 台。{}", e.message);
        }
        e
    };
    if let Some(device) = req.device {
        devices::sign_out(&session, &device).await?;
        Ok(json!({"signed_out": 1}))
    } else if req.others {
        let done = devices::sign_out_others(&session)
            .await
            .map_err(|(n, p)| partial(n, p))?;
        Ok(json!({"signed_out": done}))
    } else {
        let out = devices::sign_out_all(&session)
            .await
            .map_err(|(o, p)| partial(o.signed_out, p))?;
        Ok(json!({"signed_out": out.signed_out, "cpr_voided": out.cpr_voided}))
    }
}

#[derive(Serialize)]
struct QuotaWindow {
    window_seconds: Option<u64>,
    used_percent: Option<f64>,
    reset_at_ms: Option<i64>,
}

#[derive(Serialize)]
struct Quota {
    observed_at_ms: Option<i64>,
    windows: Vec<QuotaWindow>,
}

#[derive(Serialize)]
struct Row {
    id: String,
    name: String,
    email: Option<String>,
    plan_type: Option<String>,
    /// personal / team / unknown
    kind: &'static str,
    enabled: bool,
    credential_state: String,
    access_token_expires_at_ms: Option<i64>,
    /// 现在查不了的原因
    blocker: Option<Problem>,
    /// CPR 已记下的额度观测（不访问上游）
    quota: Option<Quota>,
    usage: Option<Summary>,
    payment: Option<LastPayment>,
}

async fn accounts(host: &HostClient) -> Result<Value, ApiError> {
    let now = now_ms();
    let list: Vec<AuthRuntimeAccount> = host::openai_accounts(host)
        .await?
        .into_iter()
        .filter(chatgpt::supported)
        .collect();
    let rows: Vec<Row> = stream::iter(list)
        .map(|a| async move {
            let (quota, usage, payment) = futures::join!(
                host::quota(host, &a.account_id),
                store::usage(host, &a.account_id),
                store::payment(host, &a.account_id),
            );
            Row {
                kind: match chatgpt::is_team(a.plan_type.as_deref()) {
                    Some(true) => "team",
                    Some(false) => "personal",
                    None => "unknown",
                },
                blocker: chatgpt::blocker(&a, now),
                quota: quota.ok().map(|q| Quota {
                    observed_at_ms: q.observed_at_ms,
                    windows: q
                        .windows
                        .into_iter()
                        .map(|w| QuotaWindow {
                            window_seconds: w.window_seconds,
                            used_percent: w.used_percent.filter(|v| v.is_finite()),
                            reset_at_ms: w.reset_at_ms,
                        })
                        .collect(),
                }),
                usage: usage.ok().flatten(),
                payment: payment.ok().flatten(),
                id: a.account_id,
                name: a.name,
                email: a.email,
                plan_type: a.plan_type,
                enabled: a.enabled,
                credential_state: a.credential_state,
                access_token_expires_at_ms: a.access_token_expires_at_ms,
            }
        })
        .buffered(LIST_CONCURRENCY)
        .collect()
        .await;
    Ok(json!({"accounts": rows, "checked_at_ms": now}))
}

async fn read_usage(host: &HostClient, id: &str, days: u32) -> Result<Value, ApiError> {
    if !usage::RANGES.contains(&days) {
        return Err(ApiError::invalid("只能查询最近 7、14 或 30 天"));
    }
    let result = match Session::open(host, id).await {
        Ok(session) => usage::read(&session, days).await,
        Err(p) => Err(p),
    };
    let summary = match &result {
        Ok(u) => Summary::of(u),
        // 账号不存在或请求本身无效时不留记录
        Err(p) if matches!(p.code, "not_found" | "invalid_range") => {
            return Err(p.clone().into());
        }
        Err(p) => Summary::failed(days, p),
    };
    // 摘要写不进去不影响这次结果
    let _ = store::save_usage(host, id, &summary).await;
    match result {
        Ok(u) => Ok(json!({"usage": usage::to_value(&u), "summary": summary})),
        Err(p) => Err(p.into()),
    }
}

async fn read_billing(host: &HostClient, id: &str, cursor: &str) -> Result<Value, ApiError> {
    let session = Session::open(host, id).await?;
    let history = billing::history(&session, cursor).await?;
    let mut payment = None;
    if cursor.is_empty() {
        payment = LastPayment::of(&history);
        if let Some(p) = &payment {
            let _ = store::save_payment(host, id, p).await;
        }
    }
    Ok(json!({"history": history, "payment": payment}))
}

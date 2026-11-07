//! ChatGPT 订阅与账单：管理员点了才查，只发 GET
//!
//! - `/backend-api/subscriptions?account_id=`：套餐、有效状态、到期、自动续费，以及
//!   付款逾期时的 `became_delinquent_timestamp` / `grace_period_end_timestamp`
//! - `/backend-api/payments/transaction-history?account_id=&limit=20[&cursor=]`：账单分页
//!
//! 发票只给 HTTPS 的 `invoice.stripe.com/i/` 链接，令牌不会发给 Stripe 或回到浏览器

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::chatgpt::{Problem, Session, encode};
use crate::clock::{now_ms, parse_rfc3339};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Subscription {
    pub plan_type: String,
    /// active / past_due / inactive / expired / free / unknown
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub will_renew: Option<bool>,
    /// 付款逾期时 ChatGPT 保留套餐到宽限期结束
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delinquent_since_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grace_ends_at_ms: Option<i64>,
    pub checked_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transaction {
    pub id: String,
    pub created_at_ms: i64,
    /// 最小货币单位，和上游一致
    pub amount: i64,
    pub currency: String,
    pub status: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub plan: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct History {
    pub transactions: Vec<Transaction>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub next_cursor: String,
    pub checked_at_ms: i64,
    /// 第一页最新一笔是扣款失败类时，顺带查的订阅（看宽限期）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subscription: Option<Subscription>,
}

/// 扣款没成功：套餐有风险
pub const FAILED: [&str; 7] = [
    "failed",
    "past_due",
    "unpaid",
    "uncollectible",
    "requires_payment_method",
    "requires_action",
    "incomplete",
];
/// 续费账单已出，还没付
pub const DUE: [&str; 3] = ["due", "open", "pending"];

/// 最近一笔账单的问题：`failed`（扣款失败）、`due`（待续费）或没有
#[must_use]
pub fn issue(status: &str) -> Option<&'static str> {
    let s = status.trim().to_ascii_lowercase();
    if FAILED.contains(&s.as_str()) {
        Some("failed")
    } else if DUE.contains(&s.as_str()) {
        Some("due")
    } else {
        None
    }
}

fn label(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= 100
        && b.iter()
            .all(|c| c.is_ascii_alphanumeric() || b"_. -".contains(c))
}

fn plan_name(s: &str) -> bool {
    label(s) && s.as_bytes()[0].is_ascii_alphabetic() && s.len() <= 80
}

/// 分页游标只放行简单字符
#[must_use]
pub fn valid_cursor(s: &str) -> bool {
    s.len() <= 1024
        && s.bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_+/=:.-".contains(&c))
}

/// 只认 Stripe 托管的发票页
#[must_use]
pub fn safe_invoice_url(raw: &str) -> bool {
    let Some(rest) = raw.strip_prefix("https://invoice.stripe.com/i/") else {
        return false;
    };
    !rest.is_empty()
        && rest.len() <= 2048
        && rest
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_-./?=&%~:+".contains(&c))
}

/// RFC 3339 或 Unix 秒；别的格式丢掉，不让整次查询失败
fn moment(v: Option<&Value>) -> Option<i64> {
    match v? {
        Value::String(s) => parse_rfc3339(s),
        Value::Number(n) => {
            let secs = n.as_f64()?;
            #[allow(clippy::cast_possible_truncation)]
            ((1e9..1e11).contains(&secs)).then_some((secs * 1000.0) as i64)
        }
        _ => None,
    }
}

/// `subscriptions` 的回答
///
/// # Errors
/// 不是预期的 JSON、账号对不上或套餐、到期时间无法识别时返回错误
pub fn parse_subscription(body: &[u8], workspace: &str, now: i64) -> Result<Subscription, Problem> {
    let bad = |m: &str| Problem::new("invalid_response", m);
    let v: Value = serde_json::from_slice(body).map_err(|_| bad("ChatGPT 返回了无法识别的数据"))?;
    let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or_default();
    let b = |k: &str| v.get(k).and_then(Value::as_bool);
    if !s("account_id").is_empty() && s("account_id") != workspace {
        return Err(Problem::new(
            "account_mismatch",
            "订阅响应与当前账号不一致，已拒绝使用",
        ));
    }
    let plan = s("plan_type");
    if !plan_name(plan) {
        return Err(bad("上游未返回可识别的套餐信息"));
    }
    let expires = match v.get("active_until") {
        None | Some(Value::Null) => None,
        Some(Value::String(t)) if t.is_empty() => None,
        Some(Value::String(t)) => {
            Some(parse_rfc3339(t).ok_or_else(|| bad("上游返回的订阅到期时间无法识别"))?)
        }
        Some(_) => return Err(bad("上游返回的订阅到期时间无法识别")),
    };
    let active = b("is_active");
    let status = if plan == "free" {
        "free"
    } else if b("is_delinquent") == Some(true) {
        "past_due"
    } else if active == Some(true) {
        "active"
    } else if active.is_some() {
        if expires.is_some_and(|e| e <= now) {
            "expired"
        } else {
            "inactive"
        }
    } else {
        match expires {
            Some(e) if e > now => "active",
            Some(_) => "expired",
            None => "unknown",
        }
    };
    Ok(Subscription {
        plan_type: plan.to_owned(),
        status: status.to_owned(),
        expires_at_ms: expires,
        will_renew: b("will_renew"),
        delinquent_since_ms: moment(v.get("became_delinquent_timestamp")),
        grace_ends_at_ms: moment(v.get("grace_period_end_timestamp")),
        checked_at_ms: now,
    })
}

/// `transaction-history` 的一页
///
/// # Errors
/// 不是预期的 JSON、字段无法识别或游标不对时返回错误
pub fn parse_history(body: &[u8], cursor: &str, now: i64) -> Result<History, Problem> {
    #[derive(Deserialize)]
    struct Product {
        #[serde(default)]
        plan: String,
    }
    #[derive(Deserialize)]
    struct Row {
        #[serde(default)]
        id: String,
        #[serde(default)]
        created_at: String,
        amount: Value,
        #[serde(default)]
        currency: String,
        #[serde(default)]
        status: String,
        #[serde(default, rename = "type")]
        kind: String,
        #[serde(default)]
        invoice_url: String,
        product: Option<Product>,
    }
    #[derive(Deserialize)]
    struct Wire {
        transactions: Option<Vec<Row>>,
        #[serde(default)]
        next_cursor: Option<String>,
    }
    let bad = |m: &str| Problem::new("invalid_response", m);
    let w: Wire =
        serde_json::from_slice(body).map_err(|_| bad("ChatGPT 返回的账单数据无法识别"))?;
    let next = w.next_cursor.unwrap_or_default();
    let Some(rows) = w.transactions else {
        return Err(bad("ChatGPT 返回的账单数据无法识别"));
    };
    if rows.len() > 100 || !valid_cursor(&next) || (!next.is_empty() && next == cursor) {
        return Err(bad("ChatGPT 返回的账单数据无法识别"));
    }
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let amount = match &r.amount {
            Value::Number(n) => n
                .as_i64()
                .or_else(|| n.as_f64().filter(|f| f.fract() == 0.0).map(|f| f as i64)),
            _ => None,
        }
        .filter(|a| a.unsigned_abs() <= 1_000_000_000_000);
        let plan = r.product.map(|p| p.plan).unwrap_or_default();
        let (Some(created), Some(amount)) = (parse_rfc3339(&r.created_at), amount) else {
            return Err(bad("ChatGPT 返回的账单字段无法识别"));
        };
        if r.currency.len() != 3
            || !r.currency.bytes().all(|c| c.is_ascii_alphabetic())
            || !label(&r.id)
            || !label(&r.status)
            || !(r.kind.is_empty() || label(&r.kind))
            || !(plan.is_empty() || label(&plan))
        {
            return Err(bad("ChatGPT 返回的账单字段无法识别"));
        }
        out.push(Transaction {
            id: r.id,
            created_at_ms: created,
            amount,
            currency: r.currency.to_ascii_uppercase(),
            status: r.status,
            kind: r.kind,
            plan,
            document_url: safe_invoice_url(&r.invoice_url).then_some(r.invoice_url),
        });
    }
    Ok(History {
        transactions: out,
        next_cursor: next,
        checked_at_ms: now,
        subscription: None,
    })
}

/// 查订阅
///
/// # Errors
/// 账号不能查、上游拒绝或回答无法识别时返回错误
pub async fn subscription(session: &Session<'_>) -> Result<Subscription, Problem> {
    let body = session
        .get(&format!(
            "/backend-api/subscriptions?account_id={}",
            encode(&session.workspace)
        ))
        .await?;
    let sub = parse_subscription(&body, &session.workspace, now_ms())?;
    if session.leaks(&sub.plan_type) {
        return Err(Problem::new(
            "invalid_response",
            "上游未返回可识别的套餐信息",
        ));
    }
    Ok(sub)
}

/// 查一页账单；第一页最新一笔是扣款失败类时顺带查订阅，看宽限期
///
/// # Errors
/// 游标无效、账号不能查、上游拒绝或回答无法识别时返回错误
pub async fn history(session: &Session<'_>, cursor: &str) -> Result<History, Problem> {
    if !valid_cursor(cursor) {
        return Err(Problem::new(
            "invalid_cursor",
            "账单分页参数无效，请重新查询",
        ));
    }
    let mut path = format!(
        "/backend-api/payments/transaction-history?account_id={}&limit=20",
        encode(&session.workspace)
    );
    if !cursor.is_empty() {
        path.push_str("&cursor=");
        path.push_str(&encode(cursor));
    }
    let body = session.get(&path).await?;
    let mut h = parse_history(&body, cursor, now_ms())?;
    let raw = serde_json::to_string(&h.transactions).unwrap_or_default();
    if session.leaks(&raw) || session.leaks(&h.next_cursor) {
        return Err(Problem::new(
            "invalid_response",
            "ChatGPT 返回的账单字段无法识别",
        ));
    }
    if cursor.is_empty()
        && newest(&h.transactions).is_some_and(|t| issue(&t.status) == Some("failed"))
    {
        // 订阅读不到不影响账单本身
        h.subscription = subscription(session).await.ok();
    }
    Ok(h)
}

/// 一页里最新的一笔
#[must_use]
pub fn newest(rows: &[Transaction]) -> Option<&Transaction> {
    rows.iter().max_by_key(|t| t.created_at_ms)
}

/// 列表上记下的最近一笔支付（不含发票链接和凭据）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LastPayment {
    pub created_at_ms: i64,
    pub amount: i64,
    pub currency: String,
    pub status: String,
    #[serde(default)]
    pub plan: String,
    pub checked_at_ms: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grace_ends_at_ms: Option<i64>,
}

impl LastPayment {
    #[must_use]
    pub fn of(h: &History) -> Option<Self> {
        let t = newest(&h.transactions)?;
        Some(Self {
            created_at_ms: t.created_at_ms,
            amount: t.amount,
            currency: t.currency.clone(),
            status: t.status.clone(),
            plan: t.plan.clone(),
            checked_at_ms: h.checked_at_ms,
            grace_ends_at_ms: h.subscription.as_ref().and_then(|s| s.grace_ends_at_ms),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000_000;

    #[test]
    fn subscription_states() {
        let p = |b: &str| parse_subscription(b.as_bytes(), "ws", NOW).unwrap();
        assert_eq!(p(r#"{"plan_type":"free"}"#).status, "free");
        let s = p(
            r#"{"account_id":"ws","plan_type":"plus","is_active":true,"is_delinquent":true,
            "active_until":"2027-01-01T00:00:00Z","became_delinquent_timestamp":1799000000,
            "grace_period_end_timestamp":"2027-01-08T00:00:00Z"}"#,
        );
        assert_eq!(s.status, "past_due");
        assert_eq!(s.delinquent_since_ms, Some(1_799_000_000_000));
        assert!(s.grace_ends_at_ms.is_some());
        assert_eq!(
            p(r#"{"plan_type":"plus","is_active":false,"active_until":"2020-01-01T00:00:00Z"}"#)
                .status,
            "expired"
        );
        assert_eq!(
            p(r#"{"plan_type":"plus","active_until":"2099-01-01T00:00:00Z"}"#).status,
            "active"
        );
        assert_eq!(p(r#"{"plan_type":"plus"}"#).status, "unknown");
        // 认不出的时间格式只丢掉那一项
        assert_eq!(
            p(r#"{"plan_type":"plus","grace_period_end_timestamp":"soon"}"#).grace_ends_at_ms,
            None
        );
    }

    #[test]
    fn subscription_rejects_other_account_and_odd_plan() {
        assert_eq!(
            parse_subscription(br#"{"account_id":"x","plan_type":"plus"}"#, "ws", NOW)
                .unwrap_err()
                .code,
            "account_mismatch"
        );
        assert!(parse_subscription(br#"{"plan_type":"<b>"}"#, "ws", NOW).is_err());
        assert!(
            parse_subscription(br#"{"plan_type":"plus","active_until":5}"#, "ws", NOW).is_err()
        );
    }

    #[test]
    fn history_page() {
        let body = br#"{"transactions":[
            {"id":"in_1","created_at":"2026-10-01T00:00:00Z","amount":2000,"currency":"usd","status":"paid","type":"invoice",
             "invoice_url":"https://invoice.stripe.com/i/acct_1/test_abc?s=ap","product":{"plan":"plus"}},
            {"id":"in_2","created_at":"2026-11-01T00:00:00Z","amount":2000.0,"currency":"usd","status":"failed",
             "invoice_url":"https://evil.example/i/x"}],"next_cursor":"c2"}"#;
        let h = parse_history(body, "", NOW).unwrap();
        assert_eq!(h.transactions.len(), 2);
        assert_eq!(h.transactions[0].currency, "USD");
        assert!(h.transactions[0].document_url.is_some());
        assert!(h.transactions[1].document_url.is_none());
        assert_eq!(newest(&h.transactions).unwrap().id, "in_2");
        let last = LastPayment::of(&h).unwrap();
        assert_eq!(last.status, "failed");
        assert_eq!(issue(&last.status), Some("failed"));
    }

    #[test]
    fn history_rejects_bad_pages() {
        assert!(parse_history(br#"{}"#, "", NOW).is_err());
        assert!(parse_history(br#"{"transactions":[],"next_cursor":"c"}"#, "c", NOW).is_err());
        assert!(
            parse_history(
                br#"{"transactions":[{"id":"x","created_at":"2026-10-01T00:00:00Z","amount":1.5,"currency":"usd","status":"paid"}]}"#,
                "",
                NOW
            )
            .is_err()
        );
        assert!(
            parse_history(
                br#"{"transactions":[{"id":"x","created_at":"2026-10-01T00:00:00Z","amount":1,"currency":"us","status":"paid"}]}"#,
                "",
                NOW
            )
            .is_err()
        );
    }

    #[test]
    fn invoice_urls() {
        assert!(safe_invoice_url("https://invoice.stripe.com/i/acct_1/abc"));
        assert!(!safe_invoice_url("http://invoice.stripe.com/i/acct_1/abc"));
        assert!(!safe_invoice_url("https://invoice.stripe.com.evil/i/x"));
        assert!(!safe_invoice_url("https://invoice.stripe.com/i/"));
        assert!(!safe_invoice_url("https://invoice.stripe.com/i/x#frag"));
        assert!(!safe_invoice_url(
            "https://invoice.stripe.com/i/x\"><script>"
        ));
    }

    #[test]
    fn issues() {
        assert_eq!(issue("Past_Due"), Some("failed"));
        assert_eq!(issue("open"), Some("due"));
        assert_eq!(issue("paid"), None);
    }
}

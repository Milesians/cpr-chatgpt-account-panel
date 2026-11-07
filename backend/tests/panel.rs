mod support;

use serde_json::{Value, json};
use support::{FakeHost, Peer, TOKEN, ok, status};

use cpr_chatgpt_account_panel::{PLUGIN_ID, config::Config, manifest, plugin};

#[test]
fn manifest_and_handlers_agree() {
    assert_eq!(manifest().unwrap().plugin_id().unwrap(), PLUGIN_ID);
    plugin(Config::default()).unwrap();
}

fn today() -> String {
    let ms = cpr_chatgpt_account_panel::clock::now_ms();
    cpr_chatgpt_account_panel::clock::date_of(ms)
}

fn limits(account: &str) -> Value {
    json!({"account_id": account, "plan_type": "plus", "rate_limit": {"allowed": true, "limit_reached": false,
        "primary_window": {"used_percent": 12, "limit_window_seconds": 18000, "reset_at": 1_900_000_000},
        "secondary_window": {"used_percent": 93, "limit_window_seconds": 604_800, "reset_at": 1_900_500_000}}})
}

fn profile() -> Value {
    json!({"stats": {"lifetime_tokens": 1000, "daily_usage_buckets": [{"start_date": today(), "tokens": 300}]}})
}

fn breakdown(work: f64) -> Value {
    json!({"units": "relative", "data": [{"date": today(), "attribution": [
        {"surface": "desktop_app", "thread_source": "user", "model": "gpt-5", "value": 3.0},
        {"surface": "work_web", "thread_source": "user", "model": "gpt-5", "value": work},
    ]}]})
}

fn assert_no_secret(body: &Value) {
    let text = body.to_string();
    assert!(
        !text.contains(TOKEN) && !text.contains("secret-refresh-token"),
        "{text}"
    );
}

#[tokio::test]
async fn list_reads_cpr_facts_without_credentials_or_upstream() {
    let mut host = FakeHost::new(&["acct_a", "acct_t"]);
    host.accounts[1]["plan_type"] = json!("team");
    host.accounts.push(json!({"account_id": "acct_k", "provider_id": "openai", "credential_revision": 1,
        "name": "key", "email": null, "upstream_user_id": null, "upstream_account_id": null, "plan_type": null,
        "authentication_kind": "api_key", "enabled": true, "credential_state": "ready", "has_refresh_token": false,
        "access_token_expires_at_ms": null, "next_refresh_at_ms": null}));
    let mut peer = Peer::start(json!({})).await;
    let (code, body) = peer.api("GET", "api/accounts", "", None, &mut host).await;
    assert_eq!(code, 200, "{body}");
    let accounts = body["accounts"].as_array().unwrap();
    assert_eq!(accounts.len(), 2, "API key accounts are not listed");
    assert_eq!(accounts[0]["kind"], "personal");
    assert_eq!(accounts[1]["kind"], "team");
    assert_eq!(accounts[0]["quota"]["windows"][0]["used_percent"], 42.5);
    assert!(accounts[0]["quota"]["windows"][1]["used_percent"].is_null());
    assert!(accounts[0]["usage"].is_null() && accounts[0]["payment"].is_null());
    assert!(
        !host.calls.iter().any(|c| c == "host.auth.get"),
        "{:?}",
        host.calls
    );
    assert!(host.seen_http.is_empty());
}

#[tokio::test]
async fn usage_reads_three_gets_through_the_account_proxy() {
    let mut host = FakeHost::new(&["acct_a"]);
    host.http.extend([
        ok(limits("chatgpt-acct_a")),
        ok(profile()),
        ok(breakdown(2.0)),
    ]);
    let mut peer = Peer::start(json!({})).await;
    let (code, body) = peer
        .api("GET", "api/usage", "id=acct_a&days=7", None, &mut host)
        .await;
    assert_eq!(code, 200, "{body}");
    assert_no_secret(&body);
    assert_eq!(host.seen_http.len(), 3);
    for r in &host.seen_http {
        assert_eq!(r.method, "GET");
        assert_eq!(r.account_id, "acct_a");
        assert_eq!(r.revision, 7);
        assert_eq!(r.body_len, 0);
        assert!(
            r.url.starts_with("https://chatgpt.com/backend-api/wham/"),
            "{}",
            r.url
        );
        assert_eq!(
            r.header("authorization").unwrap(),
            format!("Bearer {TOKEN}")
        );
        assert_eq!(r.header("chatgpt-account-id").unwrap(), "chatgpt-acct_a");
        assert_eq!(r.header("originator").unwrap(), "codex_cli_rs");
        assert!(r.header("user-agent").unwrap().starts_with("codex_cli_rs/"));
    }
    assert!(
        host.seen_http[2]
            .path()
            .starts_with("/backend-api/wham/usage/daily-token-usage-breakdown?start_date=")
    );
    let u = &body["usage"];
    assert_eq!(u["limits"]["windows"][0]["window_minutes"], 300);
    assert_eq!(u["limits"]["windows"][1]["window_minutes"], 10080);
    assert_eq!(u["work"]["total"], 2.0);
    assert_eq!(u["work"]["share"], 0.4);
    assert_eq!(u["work"]["last_day"], today());
    assert_eq!(u["tokens"]["daily"].as_array().unwrap().len(), 7);
    // 列表上记下的摘要
    let saved = host.get("usage", "acct_a").unwrap();
    assert_eq!(saved["work"]["total"], 2.0);
    assert_eq!(saved["windows"][1], json!([10080, 93.0]));
    let (_, list) = peer.api("GET", "api/accounts", "", None, &mut host).await;
    assert_eq!(list["accounts"][0]["usage"]["work"]["share"], 0.4);
}

#[tokio::test]
async fn a_team_login_reads_its_part_of_the_workspace_report() {
    let mut host = FakeHost::new(&["acct_t"]);
    host.accounts[0]["plan_type"] = json!("team");
    let mut l = limits("chatgpt-acct_t");
    l["plan_type"] = json!("team");
    host.http.extend([
        ok(l),
        ok(profile()),
        ok(json!({"units": "credits", "data": []})),
    ]);
    let mut peer = Peer::start(json!({})).await;
    let (code, body) = peer
        .api("GET", "api/usage", "id=acct_t&days=14", None, &mut host)
        .await;
    assert_eq!(code, 200, "{body}");
    assert!(
        host.seen_http[2]
            .path()
            .starts_with("/backend-api/wham/usage/daily-workspace-user-token-usage-breakdown?")
    );
    assert_eq!(body["usage"]["breakdown"]["unit"], "credits");
    assert_eq!(body["usage"]["work"]["total"], 0.0);
}

#[tokio::test]
async fn a_revoked_token_stops_the_query_and_is_remembered() {
    let mut host = FakeHost::new(&["acct_a"]);
    host.http
        .push_back(status(401, json!({"detail": "token_revoked"})));
    let mut peer = Peer::start(json!({})).await;
    let (code, body) = peer
        .api("GET", "api/usage", "id=acct_a&days=7", None, &mut host)
        .await;
    assert_eq!(code, 502, "{body}");
    assert_eq!(body["error"]["code"], "token_revoked");
    assert_eq!(host.seen_http.len(), 1, "nothing is sent after a refusal");
    assert_eq!(
        host.get("usage", "acct_a").unwrap()["error"]["code"],
        "token_revoked"
    );
}

#[tokio::test]
async fn a_missing_part_leaves_the_others() {
    let mut host = FakeHost::new(&["acct_a"]);
    host.http.extend([
        ok(limits("chatgpt-acct_a")),
        status(404, json!({})),
        ok(breakdown(0.0)),
    ]);
    let mut peer = Peer::start(json!({})).await;
    let (code, body) = peer
        .api("GET", "api/usage", "id=acct_a&days=7", None, &mut host)
        .await;
    assert_eq!(code, 200, "{body}");
    assert!(body["usage"]["tokens"].is_null());
    assert!(body["usage"]["problems"]["tokens"].is_string());
    assert_eq!(body["usage"]["work"]["total"], 0.0);
}

#[tokio::test]
async fn another_accounts_answer_is_refused() {
    let mut host = FakeHost::new(&["acct_a"]);
    host.http.push_back(ok(limits("someone-else")));
    let mut peer = Peer::start(json!({})).await;
    let (code, body) = peer
        .api("GET", "api/usage", "id=acct_a&days=7", None, &mut host)
        .await;
    assert_eq!(code, 502);
    assert_eq!(body["error"]["code"], "account_mismatch");
    assert_eq!(host.seen_http.len(), 1);
}

#[tokio::test]
async fn an_expiring_token_is_not_read_or_used() {
    let mut host = FakeHost::new(&["acct_a"]);
    host.accounts[0]["access_token_expires_at_ms"] = json!(1);
    let mut peer = Peer::start(json!({})).await;
    let (code, body) = peer
        .api("GET", "api/billing", "id=acct_a", None, &mut host)
        .await;
    assert_eq!(code, 409, "{body}");
    assert_eq!(body["error"]["code"], "token_expired");
    assert!(!host.calls.iter().any(|c| c == "host.auth.get"));
    assert!(host.seen_http.is_empty());
}

fn history(status: &str, cursor: &str) -> Value {
    json!({"transactions": [
        {"id": "in_old", "created_at": "2026-09-01T00:00:00Z", "amount": 2000, "currency": "usd", "status": "paid",
         "type": "invoice", "invoice_url": "https://invoice.stripe.com/i/acct_1/old", "product": {"plan": "plus"}},
        {"id": "in_new", "created_at": "2026-10-01T00:00:00Z", "amount": 2000, "currency": "usd", "status": status,
         "type": "invoice", "invoice_url": "https://example.com/i/x", "product": {"plan": "plus"}},
    ], "next_cursor": cursor})
}

#[tokio::test]
async fn first_billing_page_records_the_newest_payment_and_grace_period() {
    let mut host = FakeHost::new(&["acct_a"]);
    host.http.extend([
        ok(history("failed", "c2")),
        ok(json!({"account_id": "chatgpt-acct_a", "plan_type": "plus", "is_active": true, "is_delinquent": true,
            "active_until": "2026-11-01T00:00:00Z", "grace_period_end_timestamp": 1_900_000_000})),
    ]);
    let mut peer = Peer::start(json!({})).await;
    let (code, body) = peer
        .api("GET", "api/billing", "id=acct_a", None, &mut host)
        .await;
    assert_eq!(code, 200, "{body}");
    assert_no_secret(&body);
    assert_eq!(
        host.seen_http[0].path(),
        "/backend-api/payments/transaction-history?account_id=chatgpt-acct_a&limit=20"
    );
    assert_eq!(
        host.seen_http[1].path(),
        "/backend-api/subscriptions?account_id=chatgpt-acct_a"
    );
    let rows = body["history"]["transactions"].as_array().unwrap();
    assert_eq!(
        rows[0]["document_url"],
        "https://invoice.stripe.com/i/acct_1/old"
    );
    assert!(
        rows[1].get("document_url").is_none(),
        "only Stripe-hosted invoices are linked"
    );
    assert_eq!(body["history"]["subscription"]["status"], "past_due");
    let saved = host.get("payments", "acct_a").unwrap();
    assert_eq!(saved["status"], "failed");
    assert_eq!(saved["grace_ends_at_ms"], 1_900_000_000_000_i64);
    assert!(saved.get("document_url").is_none());

    // 后面的页不改记下的最近支付，也不再查订阅
    host.http
        .push_back(ok(json!({"transactions": [], "next_cursor": ""})));
    let (code, _) = peer
        .api("GET", "api/billing", "id=acct_a&cursor=c2", None, &mut host)
        .await;
    assert_eq!(code, 200);
    assert_eq!(host.seen_http.len(), 3);
    assert!(host.seen_http[2].path().ends_with("&cursor=c2"));
    assert_eq!(host.get("payments", "acct_a").unwrap()["status"], "failed");
}

#[tokio::test]
async fn a_paid_bill_does_not_look_up_the_subscription() {
    let mut host = FakeHost::new(&["acct_a"]);
    host.http.push_back(ok(history("paid", "")));
    let mut peer = Peer::start(json!({})).await;
    let (code, body) = peer
        .api("GET", "api/billing", "id=acct_a", None, &mut host)
        .await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(host.seen_http.len(), 1);
    assert!(body["history"].get("subscription").is_none());
}

#[tokio::test]
async fn challenge_pages_and_bad_input_are_reported() {
    let mut host = FakeHost::new(&["acct_a"]);
    host.http.push_back(support::HttpReply {
        status: 403,
        headers: vec![("cf-mitigated".into(), b"challenge".to_vec())],
        body: b"<html>".to_vec(),
    });
    let mut peer = Peer::start(json!({})).await;
    let (code, body) = peer
        .api("GET", "api/subscription", "id=acct_a", None, &mut host)
        .await;
    assert_eq!(code, 502);
    assert_eq!(body["error"]["code"], "upstream_challenge");
    let (code, _) = peer
        .api(
            "GET",
            "api/billing",
            "id=acct_a&cursor=%3Cx%3E",
            None,
            &mut host,
        )
        .await;
    assert_eq!(code, 400);
    let (code, _) = peer
        .api("GET", "api/usage", "id=acct_a&days=3", None, &mut host)
        .await;
    assert_eq!(code, 400);
    let (code, _) = peer
        .api("GET", "api/usage", "id=nobody&days=7", None, &mut host)
        .await;
    assert_eq!(code, 404);
    let (code, _) = peer
        .api("POST", "api/login/anything", "", Some(json!({})), &mut host)
        .await;
    assert_eq!(code, 404);
    assert_eq!(host.seen_http.len(), 1);
}

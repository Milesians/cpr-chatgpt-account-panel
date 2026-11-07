mod support;

use serde_json::{Value, json};
use support::{FakeHost, Peer, ok, status};

fn listing(rows: &[(&str, bool)]) -> support::HttpReply {
    let devices: Vec<Value> = rows
        .iter()
        .map(|(sid, current)| {
            json!({"session_id": sid, "display_name": format!("dev-{}", sid.trim_start_matches("sess_")), "platform": "macOS",
                "is_current_device": current, "is_trusted_device": false,
                "app_sessions": [{"client_type": "app", "client_name": "Codex"}]})
        })
        .collect();
    ok(json!({"devices": devices}))
}

fn revoked() -> support::HttpReply {
    ok(json!({}))
}

fn methods(host: &FakeHost) -> Vec<String> {
    host.seen_http
        .iter()
        .map(|r| format!("{} {}", r.method, r.path()))
        .collect()
}

fn revoked_sessions(host: &FakeHost) -> Vec<String> {
    host.seen_http
        .iter()
        .filter(|r| r.method == "POST")
        .map(|r| {
            assert_eq!(r.header("content-type").unwrap(), "application/json");
            let body: Value = serde_json::from_slice(&r.body).unwrap();
            body["session_id"].as_str().unwrap().to_owned()
        })
        .collect()
}

async fn device_id(
    peer: &mut Peer,
    host: &mut FakeHost,
    rows: &[(&str, bool)],
    name: &str,
) -> String {
    host.http.push_back(listing(rows));
    let (code, body) = peer
        .api("GET", "api/devices", "id=acct_a", None, host)
        .await;
    assert_eq!(code, 200, "{body}");
    body["devices"]["devices"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["name"] == name)
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[tokio::test]
async fn listing_hides_session_ids() {
    let mut host = FakeHost::new(&["acct_a"]);
    host.http
        .push_back(listing(&[("sess_cpr", true), ("sess_other", false)]));
    let mut peer = Peer::start(json!({})).await;
    let (code, body) = peer
        .api("GET", "api/devices", "id=acct_a", None, &mut host)
        .await;
    assert_eq!(code, 200, "{body}");
    assert!(!body.to_string().contains("sess_"), "{body}");
    let d = &body["devices"];
    assert_eq!(d["in_use_known"], true);
    assert_eq!(d["devices"][0]["in_use"], true);
    assert_eq!(d["devices"][1]["apps"], json!(["Codex"]));
    assert_eq!(methods(&host), vec!["GET /backend-api/accounts/sessions"]);
}

#[tokio::test]
async fn one_device_is_found_again_revoked_and_checked() {
    let mut host = FakeHost::new(&["acct_a"]);
    let mut peer = Peer::start(json!({})).await;
    let rows = [("sess_cpr", true), ("sess_phone", false)];
    let id = device_id(&mut peer, &mut host, &rows, "dev-phone").await;
    host.http
        .extend([listing(&rows), revoked(), listing(&[("sess_cpr", true)])]);
    let (code, body) = peer
        .api(
            "POST",
            "api/devices/sign-out",
            "",
            Some(json!({"id": "acct_a", "device": id})),
            &mut host,
        )
        .await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(revoked_sessions(&host), vec!["sess_phone"]);
    assert_eq!(
        methods(&host)[1..],
        [
            "GET /backend-api/accounts/sessions",
            "POST /backend-api/accounts/sessions/revoke",
            "GET /backend-api/accounts/sessions"
        ]
    );
}

#[tokio::test]
async fn cprs_own_session_cannot_be_signed_out_alone() {
    let mut host = FakeHost::new(&["acct_a"]);
    let mut peer = Peer::start(json!({})).await;
    let rows = [("sess_cpr", true), ("sess_phone", false)];
    let id = device_id(&mut peer, &mut host, &rows, "dev-cpr").await;
    host.http.push_back(listing(&rows));
    let (code, body) = peer
        .api(
            "POST",
            "api/devices/sign-out",
            "",
            Some(json!({"id": "acct_a", "device": id})),
            &mut host,
        )
        .await;
    assert_eq!(code, 409);
    assert_eq!(body["error"]["code"], "device_in_use");
    assert!(revoked_sessions(&host).is_empty());
}

#[tokio::test]
async fn others_keeps_cprs_session_and_rechecks_before_each_revoke() {
    let mut host = FakeHost::new(&["acct_a"]);
    let all = [("sess_cpr", true), ("sess_a", false), ("sess_b", false)];
    host.http.extend([
        listing(&all),
        listing(&all),
        revoked(),
        listing(&[("sess_cpr", true), ("sess_b", false)]),
        revoked(),
        listing(&[("sess_cpr", true)]),
    ]);
    let mut peer = Peer::start(json!({})).await;
    let (code, body) = peer
        .api(
            "POST",
            "api/devices/sign-out",
            "",
            Some(json!({"id": "acct_a", "others": true})),
            &mut host,
        )
        .await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(body["signed_out"], 2);
    assert_eq!(revoked_sessions(&host), vec!["sess_a", "sess_b"]);
}

#[tokio::test]
async fn others_stops_when_a_new_device_signs_in() {
    let mut host = FakeHost::new(&["acct_a"]);
    host.http.extend([
        listing(&[("sess_cpr", true), ("sess_a", false)]),
        listing(&[("sess_cpr", true), ("sess_a", false), ("sess_new", false)]),
    ]);
    let mut peer = Peer::start(json!({})).await;
    let (code, body) = peer
        .api(
            "POST",
            "api/devices/sign-out",
            "",
            Some(json!({"id": "acct_a", "others": true})),
            &mut host,
        )
        .await;
    assert_eq!(code, 409, "{body}");
    assert_eq!(body["error"]["code"], "devices_changed");
    assert!(revoked_sessions(&host).is_empty());
}

#[tokio::test]
async fn others_needs_to_know_cprs_session() {
    let mut host = FakeHost::new(&["acct_a"]);
    host.http
        .push_back(listing(&[("sess_a", false), ("sess_b", false)]));
    let mut peer = Peer::start(json!({})).await;
    let (code, body) = peer
        .api(
            "POST",
            "api/devices/sign-out",
            "",
            Some(json!({"id": "acct_a", "others": true})),
            &mut host,
        )
        .await;
    assert_eq!(code, 409);
    assert_eq!(body["error"]["code"], "in_use_unknown");
}

#[tokio::test]
async fn all_signs_out_cprs_session_last() {
    let mut host = FakeHost::new(&["acct_a"]);
    host.http.extend([
        listing(&[("sess_cpr", true), ("sess_a", false)]),
        revoked(),
        listing(&[("sess_cpr", true)]),
        revoked(),
        status(401, json!({"detail": "token_revoked"})),
    ]);
    let mut peer = Peer::start(json!({})).await;
    let (code, body) = peer
        .api(
            "POST",
            "api/devices/sign-out",
            "",
            Some(json!({"id": "acct_a", "all": true})),
            &mut host,
        )
        .await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(body["signed_out"], 2);
    assert_eq!(body["cpr_voided"], true);
    assert_eq!(revoked_sessions(&host), vec!["sess_a", "sess_cpr"]);
}

#[tokio::test]
async fn all_reports_a_token_voided_early() {
    let mut host = FakeHost::new(&["acct_a"]);
    // CPR 的会话没被认出来（没有 current，令牌也没有 session_id），跟着别的设备一起没了
    host.http.extend([
        listing(&[("sess_a", false)]),
        revoked(),
        status(401, json!({"detail": "token_revoked"})),
    ]);
    let mut peer = Peer::start(json!({})).await;
    let (code, body) = peer
        .api(
            "POST",
            "api/devices/sign-out",
            "",
            Some(json!({"id": "acct_a", "all": true})),
            &mut host,
        )
        .await;
    assert_eq!(code, 502, "{body}");
    assert_eq!(body["error"]["code"], "cpr_voided_early");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .starts_with("已登出 1 台。")
    );
}

#[tokio::test]
async fn a_sign_out_names_exactly_one_target() {
    let mut host = FakeHost::new(&["acct_a"]);
    let mut peer = Peer::start(json!({})).await;
    for body in [
        json!({"id": "acct_a"}),
        json!({"id": "acct_a", "device": "x", "all": true}),
        json!({"id": "acct_a", "others": true, "all": true}),
        json!({"id": "acct_a", "all": true, "extra": 1}),
    ] {
        let (code, _) = peer
            .api("POST", "api/devices/sign-out", "", Some(body), &mut host)
            .await;
        assert_eq!(code, 400);
    }
    assert!(host.seen_http.is_empty());
}

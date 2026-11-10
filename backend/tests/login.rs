mod support;

use serde_json::{Value, json};
use support::{FakeHost, HttpReply, Peer, TOKEN, ok, status};

use cpr_chatgpt_account_panel::login::sentinel::base64_url_encode;

const TEST_PASSWORD: &str = "MyS3cretP@ssw0rd!";
const TEST_TOTP: &str = "JBSWY3DPEHPK3PXP";
const NEW_ACCESS_TOKEN_SIG: &str = "new-secret-jwt-signature-part";
const NEW_REFRESH_TOKEN: &str = "new-secret-refresh-token-value";

fn test_config() -> Value {
    json!({
        "oauthRedirect": "http://localhost:1455/auth/callback"
    })
}

fn make_jwt(email: &str, account_id: &str, plan_type: &str) -> String {
    let header = base64_url_encode(br#"{"alg":"RS256","typ":"JWT"}"#);
    let claims = json!({
        "sub": "auth0|user_abc",
        "email": email,
        "exp": 4_102_444_800_i64,
        "https://api.openai.com/auth": {
            "chatgpt_account_id": account_id,
            "chatgpt_user_id": "user-abc",
            "chatgpt_plan_type": plan_type,
            "poid": "org-abc"
        }
    });
    let payload = base64_url_encode(&serde_json::to_vec(&claims).unwrap());
    format!("{header}.{payload}.{NEW_ACCESS_TOKEN_SIG}")
}

fn redirect(location: &str) -> HttpReply {
    HttpReply {
        status: 302,
        headers: vec![("Location".into(), location.as_bytes().to_vec())],
        body: Vec::new(),
    }
}

fn assert_no_secret_in_value(val: &Value, extra_secrets: &[&str]) {
    let s = val.to_string();
    assert!(!s.contains(TOKEN), "leaked TOKEN in {s}");
    assert!(!s.contains(TEST_PASSWORD), "leaked password in {s}");
    assert!(!s.contains(TEST_TOTP), "leaked totp in {s}");
    assert!(
        !s.contains(NEW_ACCESS_TOKEN_SIG),
        "leaked new access token in {s}"
    );
    assert!(
        !s.contains(NEW_REFRESH_TOKEN),
        "leaked new refresh token in {s}"
    );
    for &sec in extra_secrets {
        assert!(!s.contains(sec), "leaked {sec} in {s}");
    }
}

#[tokio::test]
async fn saving_credentials_stores_plaintext_in_state_and_treats_legacy_encrypted_as_none() {
    let mut host = FakeHost::new(&["acct_a"]);
    host.forbidden_secrets = vec![
        TEST_PASSWORD.to_owned(),
        TEST_TOTP.to_owned(),
        NEW_ACCESS_TOKEN_SIG.to_owned(),
        NEW_REFRESH_TOKEN.to_owned(),
    ];

    // 1. 库里已有旧加密记录（只有 passwordEnc / totpEnc，无明文 password / totp）时，反序列化自然视为未存过密码
    host.state.insert(
        ("login".to_owned(), "acct_a".to_owned()),
        (
            json!({
                "email": "acct_a@example.com",
                "kind": "personal",
                "passwordEnc": "v1.k1.oldnonce.oldcipher",
                "totpEnc": "v1.k1.oldnonce.oldtotp"
            }),
            1,
        ),
    );

    let mut peer = Peer::start(json!({})).await;
    let (code, status_body) = peer
        .api("GET", "api/login/status", "id=acct_a", None, &mut host)
        .await;
    assert_eq!(code, 200, "{status_body}");
    assert_eq!(status_body["account"]["hasPassword"], false);
    assert_eq!(status_body["account"]["hasTotp"], false);

    // 2. 不需要加密密钥即可直接保存明文密码与 2FA，且接口响应绝不回显明文
    let (code, body) = peer
        .api(
            "POST",
            "api/login/credentials",
            "",
            Some(json!({
                "accountId": "acct_a",
                "password": TEST_PASSWORD,
                "totp": TEST_TOTP,
            })),
            &mut host,
        )
        .await;
    assert_eq!(code, 200, "{body}");
    assert_no_secret_in_value(&body, &[]);
    assert_eq!(body["account"]["hasPassword"], true);
    assert_eq!(body["account"]["hasTotp"], true);

    let saved = host.get("login", "acct_a").expect("saved login record");
    assert_eq!(saved["password"], TEST_PASSWORD);
    assert_eq!(saved["totp"], TEST_TOTP);
    assert!(saved.get("passwordEnc").is_none());
    assert!(saved.get("totpEnc").is_none());
}

#[tokio::test]
async fn relogin_runs_through_the_bound_proxy_and_replaces_cpr_credentials() {
    let mut host = FakeHost::new(&["acct_a"]);
    host.forbidden_secrets = vec![
        TEST_PASSWORD.to_owned(),
        TEST_TOTP.to_owned(),
        NEW_ACCESS_TOKEN_SIG.to_owned(),
        NEW_REFRESH_TOKEN.to_owned(),
    ];

    let mut peer = Peer::start(test_config()).await;

    // 先保存明文凭据
    let (code, _) = peer
        .api(
            "POST",
            "api/login/credentials",
            "",
            Some(json!({
                "accountId": "acct_a",
                "password": TEST_PASSWORD,
                "totp": TEST_TOTP,
            })),
            &mut host,
        )
        .await;
    assert_eq!(code, 200);

    // 编排完整 OpenAI OAuth PKCE 状态机应答序列
    let session_cookie_payload = base64_url_encode(
        &serde_json::to_vec(&json!({
            "client_auth_session": {
                "workspaces": [
                    {"id": "chatgpt-acct_a", "name": "Personal"},
                    {"id": "ws-other", "name": "Other"}
                ]
            }
        }))
        .unwrap(),
    );
    let new_jwt = make_jwt("acct_a@example.com", "chatgpt-acct_a", "plus");

    host.http.extend([
        // 1. GET /oauth/authorize -> 302 /log-in
        redirect("https://auth.openai.com/log-in"),
        // 2. GET /log-in -> 200 HTML
        HttpReply {
            status: 200,
            headers: vec![
                ("content-type".into(), b"text/html".to_vec()),
                ("set-cookie".into(), b"login_session=sess1; Path=/".to_vec()),
            ],
            body: b"<html>login</html>".to_vec(),
        },
        // 3. POST sentinel/req for authorize_continue
        ok(json!({
            "token": "sent-tok-1",
            "proofofwork": {"required": true, "seed": "seed1", "difficulty": "f"}
        })),
        // 4. POST /api/accounts/authorize/continue -> password page
        ok(json!({
            "page": {"type": "password"},
            "continue_url": "/log-in/password"
        })),
        // 5. POST sentinel/req for password_verify
        ok(json!({
            "token": "sent-tok-2",
            "proofofwork": {"required": true, "seed": "seed2", "difficulty": "f"}
        })),
        // 6. POST /api/accounts/password/verify -> mfa_challenge
        ok(json!({
            "page": {"type": "mfa_challenge"},
            "mfa_factors": [{"id": "factor_totp_1", "factor_type": "totp"}]
        })),
        // 7. POST /api/accounts/mfa/issue_challenge
        ok(json!({"ok": true})),
        // 8. POST /api/accounts/mfa/verify -> consent page + session cookie
        HttpReply {
            status: 200,
            headers: vec![
                ("content-type".into(), b"application/json".to_vec()),
                (
                    "set-cookie".into(),
                    format!("oai-client-auth-session=s:{session_cookie_payload}.sig; Path=/")
                        .into_bytes(),
                ),
            ],
            body: serde_json::to_vec(&json!({
                "page": {"type": "consent"}
            }))
            .unwrap(),
        },
        // 9. POST /api/accounts/workspace/select -> 302 callback with __OAUTH_STATE__
        redirect("http://localhost:1455/auth/callback?code=oauth_code_xyz&state=__OAUTH_STATE__"),
        // 10. POST /oauth/token
        ok(json!({
            "access_token": new_jwt,
            "refresh_token": NEW_REFRESH_TOKEN,
            "id_token": ""
        })),
    ]);

    // 免输密码直接用已保存的凭据重新登录
    let (code, body) = peer
        .api(
            "POST",
            "api/login/run",
            "",
            Some(json!({"accountId": "acct_a"})),
            &mut host,
        )
        .await;
    assert_eq!(code, 200, "{body}");
    assert_no_secret_in_value(&body, &[]);
    assert_eq!(body["ok"], true);
    assert_eq!(body["account"]["accountId"], "acct_a");
    assert!(body["account"]["lastLoginAtMs"].is_i64());
    assert!(body["account"]["lastError"].is_null());

    // 证明 1：已有账号重登绝不调用 host.http.do；绑着启用的已保存代理时，全部出站请求按代理 ID
    // 经 host.http.proxy 发出，不带账号和凭据版本，CPR 中途刷新令牌不会打断登录
    assert!(
        !host.calls.iter().any(|c| c.starts_with("host.http.do")),
        "unexpected direct http calls: {:?}",
        host.calls
    );
    assert!(!host.calls.iter().any(|c| c == "host.http.account"));
    assert_eq!(host.seen_http.len(), 10);
    for req in &host.seen_http {
        assert_eq!(req.proxy_id.as_deref(), Some("proxy-acct_a"));
        assert!(req.account_id.is_empty());
    }
    assert_eq!(host.cpr_admin_calls, vec!["GET /api/admin/accounts/detail"]);
    // 校验 Sentinel PoW 令牌头和 TOTP 6 位验证码请求体
    assert!(
        host.seen_http[3]
            .header("openai-sentinel-token")
            .unwrap()
            .contains("authorize_continue")
    );
    assert!(
        host.seen_http[5]
            .header("openai-sentinel-token")
            .unwrap()
            .contains("password_verify")
    );
    let mfa_verify_body: Value = serde_json::from_slice(&host.seen_http[7].body).unwrap();
    assert_eq!(mfa_verify_body["id"], "factor_totp_1");
    assert_eq!(mfa_verify_body["code"].as_str().unwrap().len(), 6);

    // 校验自动匹配了该账号原有的 workspace_id "chatgpt-acct_a"
    let ws_select_body: Value = serde_json::from_slice(&host.seen_http[8].body).unwrap();
    assert_eq!(ws_select_body["workspace_id"], "chatgpt-acct_a");

    // 校验 host.auth.save Replace 写回了新令牌且版本号递增为 8
    assert_eq!(host.accounts[0]["credential_revision"], 8);
    let saved_facts = host.saved_credentials.get("acct_a").unwrap();
    assert_eq!(saved_facts["material"]["access_token"], new_jwt);
    assert_eq!(saved_facts["material"]["refresh_token"], NEW_REFRESH_TOKEN);
    assert_eq!(saved_facts["material"]["schema_version"], 1);
}

#[tokio::test]
async fn cloudflare_challenge_triggers_browser_challenge_and_manual_oauth_completes() {
    let mut host = FakeHost::new(&["acct_a"]);
    host.forbidden_secrets = vec![
        TEST_PASSWORD.to_owned(),
        NEW_ACCESS_TOKEN_SIG.to_owned(),
        NEW_REFRESH_TOKEN.to_owned(),
    ];

    let mut peer = Peer::start(test_config()).await;

    // 模拟上游返回 Cloudflare 403 challenge
    host.http.push_back(HttpReply {
        status: 403,
        headers: vec![("cf-mitigated".into(), b"challenge".to_vec())],
        body: b"<html>challenges.cloudflare.com</html>".to_vec(),
    });

    let (code, body) = peer
        .api(
            "POST",
            "api/login/run",
            "",
            Some(json!({
                "accountId": "acct_a",
                "password": TEST_PASSWORD,
                "saveCredentials": true,
            })),
            &mut host,
        )
        .await;
    assert_eq!(code, 403, "{body}");
    assert_eq!(body["error"]["code"], "browser_challenge");
    assert_no_secret_in_value(&body, &[]);

    // 走手动 OAuth 授权链接兜底
    let (code, start_body) = peer
        .api(
            "POST",
            "api/login/oauth/start",
            "",
            Some(json!({"accountId": "acct_a"})),
            &mut host,
        )
        .await;
    assert_eq!(code, 200, "{start_body}");
    let auth_url = start_body["authorizeUrl"].as_str().unwrap();
    let (_, query_str) = auth_url.split_once('?').unwrap();
    let q = cpr_chatgpt_account_panel::api::parse_query(query_str);
    let state = q.get("state").unwrap();

    let new_jwt = make_jwt("acct_a@example.com", "chatgpt-acct_a", "plus");
    host.http.push_back(ok(json!({
        "access_token": new_jwt,
        "refresh_token": NEW_REFRESH_TOKEN,
        "id_token": ""
    })));

    let callback_url =
        format!("http://localhost:1455/auth/callback?code=manual_code_777&state={state}");
    let (code, complete_body) = peer
        .api(
            "POST",
            "api/login/oauth/complete",
            "",
            Some(json!({
                "accountId": "acct_a",
                "callbackUrl": callback_url,
            })),
            &mut host,
        )
        .await;
    assert_eq!(code, 200, "{complete_body}");
    assert_no_secret_in_value(&complete_body, &[]);
    assert_eq!(complete_body["ok"], true);
    assert_eq!(host.accounts[0]["credential_revision"], 8);
    assert!(
        !host.calls.iter().any(|c| c.starts_with("host.http.do")),
        "unexpected direct http calls: {:?}",
        host.calls
    );
}

#[tokio::test]
async fn import_sends_through_the_chosen_proxy_and_deletes_staged_email_record() {
    let mut host = FakeHost::new(&["acct_a"]);
    host.forbidden_secrets = vec![
        TEST_PASSWORD.to_owned(),
        TEST_TOTP.to_owned(),
        "test-admin-key".to_owned(),
    ];

    let mut peer = Peer::start(test_config()).await;

    let paste_text = format!(
        "team_new@example.com----{TEST_PASSWORD}----{TEST_TOTP}----team\n\
         acct_a@example.com----{TEST_PASSWORD}--------个人\n\
         team_new@example.com----{TEST_PASSWORD}----{TEST_TOTP}----team\n"
    );

    // 1. 先预览（apply = false），不写状态、不发请求
    let (code, preview) = peer
        .api(
            "POST",
            "api/login/paste",
            "",
            Some(json!({
                "text": paste_text,
                "apply": false,
            })),
            &mut host,
        )
        .await;
    assert_eq!(code, 200, "{preview}");
    assert_no_secret_in_value(&preview, &["test-admin-key"]);
    assert_eq!(preview["applied"], false);
    assert_eq!(preview["lines"], 3);
    assert_eq!(preview["created"], 1);
    assert_eq!(preview["updated"], 1);
    assert_eq!(preview["skipped"], 1);
    assert!(host.state.is_empty());
    assert!(host.seen_http.is_empty());

    // 2. apply = true 暂存到 email:<邮箱>:<kind>
    let (code, applied) = peer
        .api(
            "POST",
            "api/login/paste",
            "",
            Some(json!({
                "text": paste_text,
                "apply": true,
            })),
            &mut host,
        )
        .await;
    assert_eq!(code, 200, "{applied}");
    assert_eq!(applied["applied"], true);
    let staged = host
        .get("login", "email:team_new@example.com:team")
        .expect("staged email record should exist before import");
    assert_eq!(staged["password"], TEST_PASSWORD);
    assert_eq!(staged["totp"], TEST_TOTP);

    // 3. 逐个导入新账号到 CPR：
    //    - 选了代理 proxy-acct_a：经 host.proxies.list 核对它启用
    //    - 再经管理接口 POST /api/admin/accounts/oauth/start（带 outboundProxyId，不带 accountId，name 为邮箱）
    //    - 经 host.http.proxy 按代理 ID 跑完登录步骤截获回调 URL（插件自己不调 /oauth/token）
    //    - 交给管理接口 POST /api/admin/accounts/oauth/complete 由 CPR 用该代理换码建号
    //    - 完成后把明文密码/2FA 存到新账号 ID 下，并用 host.state.delete 删掉 email:team_new@example.com:team 暂存记录
    host.http.extend([
        redirect("https://auth.openai.com/log-in"),
        status(200, json!({})),
        ok(json!({"token": "t1", "proofofwork": {"required": false}})),
        ok(json!({"page": {"type": "password"}})),
        ok(json!({"token": "t2", "proofofwork": {"required": false}})),
        redirect("http://127.0.0.1:1455/auth/callback?code=import_code_1&state=__OAUTH_STATE__"),
    ]);

    let (code, imported) = peer
        .api(
            "POST",
            "api/login/import",
            "",
            Some(json!({
                "email": "team_new@example.com",
                "kind": "team",
                "proxyId": "proxy-acct_a",
            })),
            &mut host,
        )
        .await;
    assert_eq!(code, 200, "{imported}");
    assert_no_secret_in_value(&imported, &["test-admin-key"]);
    assert_eq!(imported["ok"], true);
    let new_id = imported["accountId"].as_str().unwrap();
    assert_eq!(new_id, "acct_created_2");
    assert_eq!(imported["kind"], "team");
    assert_eq!(imported["hasPassword"], true);
    assert_eq!(imported["hasTotp"], true);

    // 证明 1：管理接口只经宿主内部调用（假宿主断言了相对路径、不带 Key），且只调了这几条
    assert_eq!(
        host.cpr_admin_calls,
        vec![
            "POST /api/admin/accounts/oauth/start",
            "POST /api/admin/accounts/oauth/complete",
        ]
    );
    assert_eq!(host.cpr_oauth_starts.len(), 1);
    assert_eq!(
        host.cpr_oauth_starts[0],
        json!({
            "provider": "openai",
            "name": "team_new@example.com",
            "outboundProxyId": "proxy-acct_a",
        })
    );
    assert!(host.cpr_oauth_starts[0].get("accountId").is_none());
    // 登录请求全部按所选代理的 ID 发出，不借任何账号
    assert!(!host.seen_http.is_empty());
    assert!(
        host.seen_http
            .iter()
            .all(|r| r.proxy_id.as_deref() == Some("proxy-acct_a") && r.account_id.is_empty())
    );
    assert!(!host.calls.iter().any(|c| c == "host.http.account"));
    assert_eq!(host.cpr_oauth_completes.len(), 1);
    assert_eq!(host.cpr_oauth_completes[0]["provider"], "openai");
    assert_eq!(host.cpr_oauth_completes[0]["flowId"], "flow_1");
    assert!(
        host.cpr_oauth_completes[0]["callbackUrl"]
            .as_str()
            .unwrap()
            .contains("code=import_code_1")
    );

    // 证明 2：没有调用 host.auth.save Create
    assert!(
        !host.calls.iter().any(|c| c == "host.auth.save"),
        "import must not call host.auth.save: {:?}",
        host.calls
    );

    // 证明 3：确认明文凭据已关联保存到新账号 ID 下，且暂存的 email:<邮箱>:<kind> 键已被 host.state.delete 删除
    let saved = host
        .get("login", new_id)
        .expect("saved new account login record");
    assert_eq!(saved["password"], TEST_PASSWORD);
    assert_eq!(saved["totp"], TEST_TOTP);
    assert!(
        host.get("login", "email:team_new@example.com:team")
            .is_none(),
        "staged email: key must be deleted after import"
    );
    assert!(
        host.calls.iter().any(|c| c == "host.state.delete"),
        "expected host.state.delete call"
    );

    // 再次导入同一邮箱 + 类型时返回 already_in_source
    let (code, dup) = peer
        .api(
            "POST",
            "api/login/import",
            "",
            Some(json!({
                "email": "team_new@example.com",
                "password": TEST_PASSWORD,
                "totp": TEST_TOTP,
                "kind": "team",
                "proxyId": "proxy-acct_a",
            })),
            &mut host,
        )
        .await;
    assert_eq!(code, 409, "{dup}");
    assert_eq!(dup["error"]["code"], "already_in_source");
}

async fn import(peer: &mut Peer, host: &mut FakeHost, proxy_id: &str) -> (u64, Value) {
    peer.api(
        "POST",
        "api/login/import",
        "",
        Some(json!({
            "email": "fresh@example.com",
            "password": TEST_PASSWORD,
            "kind": "personal",
            "proxyId": proxy_id,
        })),
        host,
    )
    .await
}

fn assert_nothing_sent(host: &FakeHost) {
    assert!(host.cpr_oauth_starts.is_empty());
    assert!(host.cpr_oauth_completes.is_empty());
    assert!(host.seen_http.is_empty());
    assert!(!host.calls.iter().any(|c| c == "host.auth.save"));
}

#[tokio::test]
async fn import_refuses_a_missing_or_disabled_proxy() {
    let mut host = FakeHost::new(&["acct_a", "acct_b"]);
    host.cpr_proxies[1].2 = false;
    let mut peer = Peer::start(test_config()).await;

    let (code, body) = import(&mut peer, &mut host, "").await;
    assert_eq!(code, 400, "{body}");
    assert_eq!(body["error"]["code"], "proxy_missing");

    let (code, body) = import(&mut peer, &mut host, "proxy-acct_b").await;
    assert_eq!(code, 409, "{body}");
    assert_eq!(body["error"]["code"], "proxy_unavailable");

    let (code, body) = import(&mut peer, &mut host, "proxy-gone").await;
    assert_eq!(code, 409, "{body}");
    assert_eq!(body["error"]["code"], "proxy_unavailable");

    assert_nothing_sent(&host);
}

#[tokio::test]
async fn import_works_through_a_proxy_nobody_uses_yet() {
    // 新代理没有账号绑着，CPR 里唯一的账号还停用了：照样能导入
    let mut host = FakeHost::new(&["acct_a"]);
    host.accounts[0]["enabled"] = json!(false);
    host.cpr_proxies
        .push(("proxy-new".into(), "新代理".into(), true));
    host.http.extend([
        redirect("https://auth.openai.com/log-in"),
        status(200, json!({})),
        ok(json!({"token": "t1", "proofofwork": {"required": false}})),
        ok(json!({"page": {"type": "password"}})),
        ok(json!({"token": "t2", "proofofwork": {"required": false}})),
        redirect("http://127.0.0.1:1455/auth/callback?code=new_code&state=__OAUTH_STATE__"),
    ]);
    let mut peer = Peer::start(test_config()).await;
    let (code, body) = import(&mut peer, &mut host, "proxy-new").await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(host.cpr_oauth_starts[0]["outboundProxyId"], "proxy-new");
    assert_eq!(host.seen_http.len(), 6);
    assert!(
        host.seen_http
            .iter()
            .all(|r| r.proxy_id.as_deref() == Some("proxy-new"))
    );
    assert_eq!(
        host.cpr_account_proxies[body["accountId"].as_str().unwrap()].as_deref(),
        Some("proxy-new")
    );
}

#[tokio::test]
async fn import_reports_a_proxy_disabled_mid_login() {
    // 核对时还启用，发请求时宿主说已停用：不回退直连，报 proxy_unavailable
    let mut host = FakeHost::new(&["acct_a"]);
    host.cpr_proxies
        .push(("proxy-flaky".into(), "会停用的代理".into(), true));
    let mut peer = Peer::start(test_config()).await;
    // oauth/start 之后、第一条登录请求之前停用
    host.disable_proxy_before_http = Some("proxy-flaky".into());
    let (code, body) = import(&mut peer, &mut host, "proxy-flaky").await;
    assert_eq!(code, 409, "{body}");
    assert_eq!(body["error"]["code"], "proxy_unavailable");
    assert!(host.seen_http.is_empty());
    assert!(host.cpr_oauth_completes.is_empty());
}

#[tokio::test]
async fn browser_login_can_use_a_proxy_nobody_uses_yet() {
    let mut host = FakeHost::new(&["acct_a"]);
    host.cpr_proxies
        .push(("proxy-new".into(), "新代理".into(), true));
    let mut peer = Peer::start(test_config()).await;
    let (code, body) = peer
        .api(
            "POST",
            "api/login/oauth/start",
            "",
            Some(json!({"email": "fresh@example.com", "kind": "personal", "proxyId": "proxy-new"})),
            &mut host,
        )
        .await;
    assert_eq!(code, 200, "{body}");
    // CPR 给的是桌面登录入口；回调地址要用它内层的 redirect_uri，而不是插件配置的 localhost
    assert!(
        body["authorizeUrl"]
            .as_str()
            .unwrap()
            .starts_with("https://chatgpt.com/codex/desktop-auth?authorize_url=")
    );
    assert_eq!(body["redirectUrl"], "http://127.0.0.1:1455/auth/callback");
    assert_eq!(host.cpr_oauth_starts[0]["outboundProxyId"], "proxy-new");
    assert!(
        host.seen_http.is_empty(),
        "browser login sends nothing itself"
    );

    // 浏览器里登录完，把 127.0.0.1 的回调链接贴回来；state 取自内层链接
    let state = host.last_oauth_state.clone();
    let (code, done) = peer
        .api(
            "POST",
            "api/login/oauth/complete",
            "",
            Some(json!({
                "email": "fresh@example.com",
                "kind": "personal",
                "callbackUrl": format!("http://127.0.0.1:1455/auth/callback?code=browser_code&state={state}"),
            })),
            &mut host,
        )
        .await;
    assert_eq!(code, 200, "{done}");
    assert_eq!(host.cpr_oauth_completes.len(), 1);
    assert!(
        host.seen_http.is_empty(),
        "browser login sends nothing itself"
    );
}

#[tokio::test]
async fn proxy_list_comes_from_the_host_directory_without_addresses() {
    let mut host = FakeHost::new(&["acct_a", "acct_b"]);
    host.cpr_proxies[1].2 = false;
    host.cpr_proxies
        .push(("proxy-new".into(), "新代理".into(), true));
    let mut peer = Peer::start(test_config()).await;
    let (code, body) = peer
        .api("GET", "api/login/proxies", "", None, &mut host)
        .await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(
        body["proxies"],
        json!([
            {"id": "proxy-acct_a", "name": "代理 acct_a", "enabled": true},
            {"id": "proxy-acct_b", "name": "代理 acct_b", "enabled": false},
            {"id": "proxy-new", "name": "新代理", "enabled": true},
        ])
    );
    assert!(host.calls.iter().any(|c| c == "host.proxies.list"));
    assert!(
        host.cpr_admin_calls.is_empty(),
        "{:?}",
        host.cpr_admin_calls
    );
}

#[tokio::test]
async fn relogin_uses_the_account_egress_when_no_enabled_saved_proxy() {
    // acct_a 没绑代理（CPR 直连）；acct_b 绑的代理停用了（CPR 也改直连）：都经账号自己的 host.http.account
    let mut host = FakeHost::new(&["acct_a", "acct_b"]);
    host.cpr_account_proxies.insert("acct_a".into(), None);
    host.cpr_proxies[1].2 = false;
    let mut peer = Peer::start(test_config()).await;
    for id in ["acct_a", "acct_b"] {
        host.http.push_back(HttpReply {
            status: 403,
            headers: vec![("cf-mitigated".into(), b"challenge".to_vec())],
            body: b"<html>challenges.cloudflare.com</html>".to_vec(),
        });
        let (code, body) = peer
            .api(
                "POST",
                "api/login/run",
                "",
                Some(json!({"accountId": id, "password": TEST_PASSWORD})),
                &mut host,
            )
            .await;
        assert_eq!(code, 403, "{body}");
        let req = host.seen_http.last().unwrap();
        assert_eq!(req.account_id, id);
        assert_eq!(req.revision, 7);
        assert!(req.proxy_id.is_none());
    }
    assert!(!host.calls.iter().any(|c| c == "host.http.proxy"));
}

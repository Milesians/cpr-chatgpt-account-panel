mod support;

use serde_json::{Value, json};
use support::{FakeHost, HttpReply, Peer, TOKEN, ok, status};

use cpr_chatgpt_account_panel::login::crypto::{Keyring, base64_url_encode, credential_aad};

const TEST_KEY_RAW: [u8; 32] = [9u8; 32];
const TEST_PASSWORD: &str = "MyS3cretP@ssw0rd!";
const TEST_TOTP: &str = "JBSWY3DPEHPK3PXP";
const NEW_ACCESS_TOKEN_SIG: &str = "new-secret-jwt-signature-part";
const NEW_REFRESH_TOKEN: &str = "new-secret-refresh-token-value";

fn test_config() -> Value {
    let key_b64 = base64_url_encode(&TEST_KEY_RAW);
    json!({
        "encryptionKey": format!("k1:{key_b64}"),
        "activeKey": "k1",
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
async fn saving_credentials_requires_encryption_key_and_encrypts_with_aad_in_state() {
    let mut host = FakeHost::new(&["acct_a"]);
    host.forbidden_secrets = vec![
        TEST_PASSWORD.to_owned(),
        TEST_TOTP.to_owned(),
        NEW_ACCESS_TOKEN_SIG.to_owned(),
        NEW_REFRESH_TOKEN.to_owned(),
    ];

    // 1. 未配置加密密钥时拒绝保存凭据
    let mut peer_no_key = Peer::start(json!({})).await;
    let (code, body) = peer_no_key
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
    assert_eq!(code, 400, "{body}");
    assert_eq!(body["error"]["code"], "encryption_key_missing");
    assert!(host.get("login", "acct_a").is_none());

    // 2. 配置加密密钥后，用 AES-256-GCM 加密写入私有状态，且绑定 AAD
    let key_b64 = base64_url_encode(&TEST_KEY_RAW);
    host.forbidden_secrets.push(key_b64.clone());
    let mut peer = Peer::start(test_config()).await;
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
    assert_no_secret_in_value(&body, &[&key_b64]);
    assert_eq!(body["account"]["hasPassword"], true);
    assert_eq!(body["account"]["hasTotp"], true);

    let saved = host.get("login", "acct_a").expect("saved login record");
    let pw_enc = saved["passwordEnc"].as_str().unwrap();
    let totp_enc = saved["totpEnc"].as_str().unwrap();
    assert!(pw_enc.starts_with("v1.k1."));
    assert!(totp_enc.starts_with("v1.k1."));

    let kr = Keyring::from_config(&format!("k1:{key_b64}"), "k1");
    assert_eq!(
        kr.open(pw_enc, &credential_aad("acct_a", "password"))
            .unwrap(),
        TEST_PASSWORD
    );
    assert_eq!(
        kr.open(totp_enc, &credential_aad("acct_a", "totp"))
            .unwrap(),
        TEST_TOTP
    );
    // 跨账号或跨字段 AAD 校验必须失败
    assert!(
        kr.open(pw_enc, &credential_aad("acct_b", "password"))
            .is_err()
    );
    assert!(kr.open(pw_enc, &credential_aad("acct_a", "totp")).is_err());
}

#[tokio::test]
async fn full_login_flow_runs_strictly_through_account_http_and_replaces_cpr_credentials() {
    let mut host = FakeHost::new(&["acct_a"]);
    let key_b64 = base64_url_encode(&TEST_KEY_RAW);
    host.forbidden_secrets = vec![
        TEST_PASSWORD.to_owned(),
        TEST_TOTP.to_owned(),
        key_b64.clone(),
        NEW_ACCESS_TOKEN_SIG.to_owned(),
        NEW_REFRESH_TOKEN.to_owned(),
    ];

    let mut peer = Peer::start(test_config()).await;

    // 先保存加密凭据
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

    // 免输密码直接用已保存的加密凭据重新登录
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
    assert_no_secret_in_value(&body, &[&key_b64]);
    assert_eq!(body["ok"], true);
    assert_eq!(body["account"]["accountId"], "acct_a");
    assert!(body["account"]["lastLoginAtMs"].is_i64());
    assert!(body["account"]["lastError"].is_null());

    // 证明 1：绝不调用 host.http.do，全部出站请求只经 host.http.account
    assert!(
        !host.calls.iter().any(|c| c.starts_with("host.http.do")),
        "unexpected direct http calls: {:?}",
        host.calls
    );
    assert_eq!(host.seen_http.len(), 10);
    for req in &host.seen_http {
        assert_eq!(req.account_id, "acct_a");
        assert_eq!(req.revision, 7);
    }
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
    let key_b64 = base64_url_encode(&TEST_KEY_RAW);
    host.forbidden_secrets = vec![
        TEST_PASSWORD.to_owned(),
        key_b64.clone(),
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
    assert_no_secret_in_value(&body, &[&key_b64]);

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
    assert_no_secret_in_value(&complete_body, &[&key_b64]);
    assert_eq!(complete_body["ok"], true);
    assert_eq!(host.accounts[0]["credential_revision"], 8);
    assert!(
        !host.calls.iter().any(|c| c.starts_with("host.http.do")),
        "unexpected direct http calls: {:?}",
        host.calls
    );
}

#[tokio::test]
async fn paste_preview_and_quick_import_create_account_and_save_encrypted_secrets() {
    let mut host = FakeHost::new(&["acct_a"]);
    let key_b64 = base64_url_encode(&TEST_KEY_RAW);
    host.forbidden_secrets = vec![
        TEST_PASSWORD.to_owned(),
        TEST_TOTP.to_owned(),
        key_b64.clone(),
        NEW_ACCESS_TOKEN_SIG.to_owned(),
        NEW_REFRESH_TOKEN.to_owned(),
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
    assert_no_secret_in_value(&preview, &[&key_b64]);
    assert_eq!(preview["applied"], false);
    assert_eq!(preview["lines"], 3);
    assert_eq!(preview["created"], 1);
    assert_eq!(preview["updated"], 1);
    assert_eq!(preview["skipped"], 1);
    assert!(host.state.is_empty());
    assert!(host.seen_http.is_empty());

    // 2. 逐个导入新账号到 CPR（借用启用中的 acct_a 代理走 host.http.account）
    let new_jwt = make_jwt("team_new@example.com", "ws-team-99", "team");
    host.http.extend([
        redirect("https://auth.openai.com/log-in"),
        status(200, json!({})),
        ok(json!({"token": "t1", "proofofwork": {"required": false}})),
        ok(json!({"page": {"type": "password"}})),
        ok(json!({"token": "t2", "proofofwork": {"required": false}})),
        redirect("http://localhost:1455/auth/callback?code=import_code_1&state=__OAUTH_STATE__"),
        ok(json!({
            "access_token": new_jwt,
            "refresh_token": NEW_REFRESH_TOKEN,
            "id_token": ""
        })),
    ]);

    let (code, imported) = peer
        .api(
            "POST",
            "api/login/import",
            "",
            Some(json!({
                "email": "team_new@example.com",
                "password": TEST_PASSWORD,
                "totp": TEST_TOTP,
                "kind": "team",
                "viaAccountId": "acct_a",
            })),
            &mut host,
        )
        .await;
    assert_eq!(code, 200, "{imported}");
    assert_no_secret_in_value(&imported, &[&key_b64]);
    assert_eq!(imported["ok"], true);
    let new_id = imported["accountId"].as_str().unwrap();
    assert_eq!(new_id, "acct_created_2");
    assert_eq!(imported["kind"], "team");
    assert_eq!(imported["hasPassword"], true);
    assert_eq!(imported["hasTotp"], true);

    // 确认加密后的凭据已关联保存到新账号 ID 下
    let saved = host
        .get("login", new_id)
        .expect("saved new account login record");
    let kr = Keyring::from_config(&format!("k1:{key_b64}"), "k1");
    assert_eq!(
        kr.open(
            saved["passwordEnc"].as_str().unwrap(),
            &credential_aad(new_id, "password")
        )
        .unwrap(),
        TEST_PASSWORD
    );
    assert_eq!(
        kr.open(
            saved["totpEnc"].as_str().unwrap(),
            &credential_aad(new_id, "totp")
        )
        .unwrap(),
        TEST_TOTP
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
                "viaAccountId": "acct_a",
            })),
            &mut host,
        )
        .await;
    assert_eq!(code, 409, "{dup}");
    assert_eq!(dup["error"]["code"], "already_in_source");
    assert!(
        !host.calls.iter().any(|c| c.starts_with("host.http.do")),
        "unexpected direct http calls: {:?}",
        host.calls
    );
}

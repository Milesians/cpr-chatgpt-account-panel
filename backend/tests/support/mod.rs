#![allow(dead_code)]
//! 经公开 RPC 会话驱动插件，模拟宿主回调：账号、凭据、私有状态、额度观测和受管账号 HTTP

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::time::Duration;

use cpr_chatgpt_account_panel::{PLUGIN_ID, manifest, serve};
use gateway_plugin_sdk::{
    CallContext, ErrorCode, Frame, Handshake, Message, PROTOCOL_VERSION, PluginFault,
    ResolvedCallBudgets, Stage,
    client::{read_frame, write_frame},
};
use serde_json::{Value, json};
use tokio::{
    io::{DuplexStream, ReadHalf, WriteHalf},
    task::JoinHandle,
};

pub type Reply = Result<(Value, Vec<u8>), PluginFault>;

pub const TOKEN: &str = "secret-access-token";

/// 账号 HTTP 的一次应答
pub struct HttpReply {
    pub status: u16,
    pub headers: Vec<(String, Vec<u8>)>,
    pub body: Vec<u8>,
}

pub fn ok(body: Value) -> HttpReply {
    HttpReply {
        status: 200,
        headers: vec![("content-type".into(), b"application/json".to_vec())],
        body: serde_json::to_vec(&body).unwrap(),
    }
}

pub fn status(status: u16, body: Value) -> HttpReply {
    HttpReply {
        status,
        headers: Vec::new(),
        body: serde_json::to_vec(&body).unwrap(),
    }
}

/// 一次 `host.http.account` 收到的请求
#[allow(dead_code)]
pub struct SeenRequest {
    pub account_id: String,
    pub revision: u64,
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, Vec<u8>)>,
    pub body_len: usize,
    pub body: Vec<u8>,
}

#[allow(dead_code)]
impl SeenRequest {
    pub fn header(&self, name: &str) -> Option<String> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| String::from_utf8(v.clone()).unwrap())
    }

    pub fn path(&self) -> &str {
        self.url.trim_start_matches("https://chatgpt.com")
    }
}

#[derive(Default)]
#[allow(dead_code)]
pub struct FakeHost {
    pub state: HashMap<(String, String), (Value, u64)>,
    pub accounts: Vec<Value>,
    pub saved_credentials: HashMap<String, Value>,
    pub forbidden_secrets: Vec<String>,
    pub http: VecDeque<HttpReply>,
    pub seen_http: Vec<SeenRequest>,
    pub calls: Vec<String>,
    pub last_oauth_state: String,
    /// 假的 CPR 管理接口（`http://cpr.test`）
    pub cpr_admin_key: String,
    pub cpr_account_proxies: HashMap<String, Option<String>>,
    pub cpr_admin_calls: Vec<String>,
    pub cpr_oauth_starts: Vec<Value>,
    pub cpr_oauth_completes: Vec<Value>,
    pub cpr_next_plan_type: Option<String>,
    cpr_pending_flows: HashMap<String, (String, String, String)>,
    streams: HashMap<String, Vec<u8>>,
    next_version: u64,
}

pub fn account(id: &str) -> Value {
    json!({
        "account_id": id, "provider_id": "openai", "credential_revision": 7,
        "name": format!("name-{id}"), "email": format!("{id}@example.com"),
        "upstream_user_id": null, "upstream_account_id": format!("chatgpt-{id}"),
        "plan_type": "plus", "authentication_kind": "oauth", "enabled": true,
        "credential_state": "ready", "has_refresh_token": true,
        "access_token_expires_at_ms": 4_102_444_800_000_i64, "next_refresh_at_ms": null,
    })
}

#[allow(dead_code)]
impl FakeHost {
    pub fn new(accounts: &[&str]) -> Self {
        let mut cpr_account_proxies = HashMap::new();
        for &id in accounts {
            cpr_account_proxies.insert(id.to_owned(), Some(format!("proxy-{id}")));
        }
        Self {
            accounts: accounts.iter().map(|id| account(id)).collect(),
            cpr_admin_key: "test-admin-key".to_owned(),
            cpr_account_proxies,
            ..Self::default()
        }
    }

    pub fn get(&self, namespace: &str, key: &str) -> Option<&Value> {
        self.state
            .get(&(namespace.to_owned(), key.to_owned()))
            .map(|(v, _)| v)
    }

    fn check_no_leaks(&self, text: &str, target: &str) {
        assert!(!text.contains(TOKEN), "a credential reached {target}");
        for secret in &self.forbidden_secrets {
            assert!(
                !secret.is_empty() && !text.contains(secret),
                "secret {secret} reached {target}: {text}"
            );
        }
    }

    fn cpr_admin(&mut self, params: &Value, payload: &[u8]) -> (u16, Value) {
        let method = params["method"].as_str().unwrap().to_owned();
        let url = params["url"].as_str().unwrap();
        let path = url
            .trim_start_matches("http://cpr.test")
            .split('?')
            .next()
            .unwrap()
            .to_owned();
        self.cpr_admin_calls.push(format!("{method} {path}"));
        let key = params["headers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|h| h[0].as_str().unwrap().eq_ignore_ascii_case("x-api-key"))
            .map(|h| {
                let bytes: Vec<u8> = serde_json::from_value(h[1].clone()).unwrap();
                String::from_utf8(bytes).unwrap()
            });
        if key.as_deref() != Some(self.cpr_admin_key.as_str()) || self.cpr_admin_key.is_empty() {
            return (
                401,
                json!({"code": 401, "message": "invalid admin api key", "data": null}),
            );
        }
        let ok = |data: Value| (200, json!({"code": 200, "message": "ok", "data": data}));
        match (method.as_str(), path.as_str()) {
            ("GET", "/api/admin/accounts") => {
                assert!(url.contains("provider=openai"));
                let items: Vec<Value> = self
                    .accounts
                    .iter()
                    .map(|a| {
                        let id = a["account_id"].as_str().unwrap();
                        let proxy = self.cpr_account_proxies.get(id).cloned().unwrap_or(None);
                        json!({
                            "id": id,
                            "name": a["name"],
                            "email": a["email"],
                            "planType": a["plan_type"],
                            "provider": "openai",
                            "outboundProxyId": proxy,
                        })
                    })
                    .collect();
                ok(json!({"items": items, "page": {"totalPages": 1}}))
            }
            ("POST", "/api/admin/accounts/oauth/start") => {
                let body: Value = serde_json::from_slice(payload).unwrap();
                self.cpr_oauth_starts.push(body.clone());
                assert_eq!(body["provider"], "openai");
                assert!(
                    body.get("accountId").is_none(),
                    "oauth/start for new import must not send accountId: {body}"
                );
                let name = body["name"].as_str().unwrap().to_owned();
                let proxy_id = body["outboundProxyId"].as_str().unwrap().to_owned();
                assert!(!proxy_id.is_empty(), "outboundProxyId must be non-empty");
                let idx = self.cpr_oauth_starts.len();
                let flow_id = format!("flow_{idx}");
                let state = format!("cpr_oauth_state_{idx}");
                self.last_oauth_state.clone_from(&state);
                let auth_url = format!(
                    "https://auth.openai.com/oauth/authorize?client_id=app_EMoamEEZ73f0CkXaXp7hrann&code_challenge=ch_{idx}&code_challenge_method=S256&redirect_uri=http%3A%2F%2Flocalhost%3A1455%2Fauth%2Fcallback&response_type=code&scope=openid+profile+email+offline_access&state={state}"
                );
                self.cpr_pending_flows
                    .insert(flow_id.clone(), (name, proxy_id, state));
                ok(json!({
                    "flowId": flow_id,
                    "authorizationUrl": auth_url,
                    "expiresAt": "2026-11-07T12:00:00Z",
                }))
            }
            ("POST", "/api/admin/accounts/oauth/complete") => {
                let body: Value = serde_json::from_slice(payload).unwrap();
                self.cpr_oauth_completes.push(body.clone());
                assert_eq!(body["provider"], "openai");
                let flow_id = body["flowId"].as_str().unwrap();
                let callback_url = body["callbackUrl"].as_str().unwrap();
                let (email, proxy_id, expected_state) = self
                    .cpr_pending_flows
                    .remove(flow_id)
                    .expect("unknown or already completed flowId");
                assert!(
                    callback_url.contains(&format!("state={expected_state}")),
                    "callbackUrl {callback_url} did not contain expected state {expected_state}"
                );
                assert!(
                    callback_url.contains("code="),
                    "callbackUrl {callback_url} did not contain code"
                );
                let id = format!("acct_created_{}", self.accounts.len() + 1);
                let plan_type = self
                    .cpr_next_plan_type
                    .clone()
                    .unwrap_or_else(|| "team".to_owned());
                self.accounts.push(json!({
                    "account_id": id,
                    "provider_id": "openai",
                    "credential_revision": 1,
                    "name": email,
                    "email": email,
                    "upstream_user_id": "user-imported",
                    "upstream_account_id": format!("chatgpt-{id}"),
                    "plan_type": plan_type,
                    "authentication_kind": "oauth",
                    "enabled": true,
                    "credential_state": "ready",
                    "has_refresh_token": true,
                    "access_token_expires_at_ms": 4_102_444_800_000_i64,
                    "next_refresh_at_ms": null,
                }));
                self.cpr_account_proxies.insert(id.clone(), Some(proxy_id));
                ok(json!({ "accountId": id }))
            }
            _ => panic!("plugin called an unexpected CPR admin route: {method} {path}"),
        }
    }

    fn find(&self, payload: &[u8]) -> Result<Value, PluginFault> {
        let id = serde_json::from_slice::<Value>(payload).unwrap()["account_id"]
            .as_str()
            .unwrap()
            .to_owned();
        self.accounts
            .iter()
            .find(|a| a["account_id"] == id.as_str())
            .cloned()
            .ok_or_else(|| PluginFault::new(ErrorCode::Rejected, "not found"))
    }

    pub fn handle(&mut self, method: &str, params: &Value, payload: &[u8]) -> Reply {
        self.calls.push(method.to_owned());
        match method {
            "host.auth.list" => {
                let req: Value = serde_json::from_slice(payload).unwrap();
                assert_eq!(req["provider_id"], "openai");
                let body = json!({"accounts": self.accounts, "next_cursor": null});
                Ok((json!({}), serde_json::to_vec(&body).unwrap()))
            }
            "host.auth.get_runtime" => {
                let a = self.find(payload)?;
                Ok((json!({}), serde_json::to_vec(&a).unwrap()))
            }
            "host.auth.get" => {
                let a = self.find(payload)?;
                let id = a["account_id"].as_str().unwrap();
                let facts = self.saved_credentials.get(id).cloned().unwrap_or_else(|| {
                    json!({
                        "name": a["name"], "authentication_kind": "oauth",
                        "material": {"access_token": TOKEN, "refresh_token": "secret-refresh-token"},
                        "email": a["email"], "upstream_user_id": null, "upstream_account_id": a["upstream_account_id"],
                        "plan_type": a["plan_type"], "has_refresh_token": true,
                        "access_token_expires_at_ms": a["access_token_expires_at_ms"], "next_refresh_at_ms": null,
                    })
                });
                let body = json!({
                    "account_id": a["account_id"], "provider_id": "openai", "credential_revision": a["credential_revision"],
                    "facts": facts,
                });
                Ok((json!({}), serde_json::to_vec(&body).unwrap()))
            }
            "host.auth.save" => {
                use gateway_plugin_sdk::call::host::AuthSaveRequest;
                let req: AuthSaveRequest = serde_json::from_slice(payload).unwrap();
                match req {
                    AuthSaveRequest::Replace {
                        account_id: id,
                        credential_revision: expected_rev,
                        facts,
                    } => {
                        let facts_val = serde_json::to_value(&facts).unwrap();
                        let acct = self
                            .accounts
                            .iter_mut()
                            .find(|a| a["account_id"] == id.as_str())
                            .ok_or_else(|| PluginFault::new(ErrorCode::Rejected, "not found"))?;
                        if acct["credential_revision"].as_u64().unwrap() != expected_rev {
                            return Err(PluginFault::new(ErrorCode::Conflict, "revision conflict"));
                        }
                        let new_rev = expected_rev + 1;
                        acct["credential_revision"] = json!(new_rev);
                        acct["name"] = facts_val["name"].clone();
                        acct["email"] = facts_val["email"].clone();
                        acct["upstream_user_id"] = facts_val["upstream_user_id"].clone();
                        acct["upstream_account_id"] = facts_val["upstream_account_id"].clone();
                        acct["plan_type"] = facts_val["plan_type"].clone();
                        acct["has_refresh_token"] = facts_val["has_refresh_token"].clone();
                        acct["access_token_expires_at_ms"] =
                            facts_val["access_token_expires_at_ms"].clone();
                        acct["credential_state"] = json!("ready");
                        self.saved_credentials.insert(id.clone(), facts_val);
                        let out = json!({"account_id": id, "credential_revision": new_rev});
                        Ok((json!({}), serde_json::to_vec(&out).unwrap()))
                    }
                    AuthSaveRequest::Create { .. } => {
                        panic!(
                            "host.auth.save Create should not be called; use CPR Admin oauth/start + oauth/complete instead"
                        );
                    }
                }
            }
            "host.data.quota.get" => {
                let a = self.find(payload)?;
                let body = json!({
                    "schema_version": 1, "account_id": a["account_id"], "observed_at_ms": 1_800_000_000_000_i64,
                    "windows": [
                        {"key": "weekly", "window_seconds": 604_800, "used_percent": 42.5, "reset_at_ms": 1_800_100_000_000_i64},
                        {"key": "five_hour", "window_seconds": 18_000, "used_percent": null, "reset_at_ms": null},
                    ],
                });
                Ok((json!({}), serde_json::to_vec(&body).unwrap()))
            }
            "host.state.get" => {
                let key = (
                    params["namespace"].as_str().unwrap().to_owned(),
                    params["key"].as_str().unwrap().to_owned(),
                );
                let record = self
                    .state
                    .get(&key)
                    .map(|(v, ver)| json!({"value": v, "version": ver, "schema_version": 1}));
                Ok((json!({"record": record}), Vec::new()))
            }
            "host.state.put" => {
                let key = (
                    params["namespace"].as_str().unwrap().to_owned(),
                    params["key"].as_str().unwrap().to_owned(),
                );
                let manifest = manifest().unwrap();
                let declared = manifest
                    .state
                    .iter()
                    .find(|s| s.namespace == key.0)
                    .unwrap_or_else(|| panic!("undeclared namespace {}", key.0));
                let expected = params["expected_version"].as_u64();
                let current = self.state.get(&key).map(|(_, v)| *v);
                if current != expected {
                    return Err(PluginFault::new(ErrorCode::Conflict, "version"));
                }
                let text = serde_json::to_string(&params["value"]).unwrap();
                assert!(
                    text.len() <= declared.maximum_value_bytes as usize,
                    "{} {} is {} bytes",
                    key.0,
                    key.1,
                    text.len()
                );
                // 对 usage、payments 两个命名空间保留“访问令牌不能进来”的断言；
                // 对 login 命名空间放开登录凭据（但仍不允许访问令牌进入）
                assert!(!text.contains(TOKEN), "a credential reached private state");
                if key.0 != "login" {
                    self.check_no_leaks(&text, "private state");
                }
                self.next_version += 1;
                self.state
                    .insert(key, (params["value"].clone(), self.next_version));
                Ok((json!({"version": self.next_version}), Vec::new()))
            }
            "host.state.delete" => {
                let key = (
                    params["namespace"].as_str().unwrap().to_owned(),
                    params["key"].as_str().unwrap().to_owned(),
                );
                let expected = params["expected_version"].as_u64().unwrap();
                match self.state.get(&key) {
                    Some((_, ver)) if *ver != expected => {
                        return Err(PluginFault::new(ErrorCode::Conflict, "version"));
                    }
                    None => return Ok((json!({"deleted": false}), Vec::new())),
                    _ => {}
                }
                let deleted = self.state.remove(&key).is_some();
                Ok((json!({"deleted": deleted}), Vec::new()))
            }
            "host.services.call" => {
                assert_eq!(
                    params,
                    &json!({"operation":"settings.preview_client_profile","input":["openai",null]})
                );
                let profile = json!({
                    "userAgent":"codex_cli_rs/0.156.1 (Ubuntu 24.4.0; x86_64) xterm-256color",
                    "originator":"codex_cli_rs", "codexVersion":"0.156.1", "source":"global"
                });
                Ok((json!({"Ok":profile}), Vec::new()))
            }
            "host.http.do_stream"
                if params["url"]
                    .as_str()
                    .is_some_and(|u| u.starts_with("http://cpr.test/")) =>
            {
                let (status, body) = self.cpr_admin(params, payload);
                let id = format!("cpr-stream-{}", self.cpr_admin_calls.len());
                self.streams
                    .insert(id.clone(), serde_json::to_vec(&body).unwrap());
                Ok((
                    json!({"status": status, "http_version": "HTTP/1.1", "headers": [], "stream": id, "trailers": []}),
                    Vec::new(),
                ))
            }
            "host.http.account" => {
                let mut reply = self
                    .http
                    .pop_front()
                    .expect("unscripted account HTTP request");
                let url = params["request"]["url"].as_str().unwrap().to_owned();
                if let Some((_, q)) = url.split_once('?') {
                    let parsed = cpr_chatgpt_account_panel::api::parse_query(q);
                    if let Some(st) = parsed.get("state") {
                        self.last_oauth_state.clone_from(st);
                    }
                }
                if !self.last_oauth_state.is_empty() {
                    for (_, v) in &mut reply.headers {
                        if let Ok(s) = std::str::from_utf8(v)
                            && s.contains("__OAUTH_STATE__")
                        {
                            *v = s
                                .replace("__OAUTH_STATE__", &self.last_oauth_state)
                                .into_bytes();
                        }
                    }
                    if let Ok(s) = std::str::from_utf8(&reply.body)
                        && s.contains("__OAUTH_STATE__")
                    {
                        reply.body = s
                            .replace("__OAUTH_STATE__", &self.last_oauth_state)
                            .into_bytes();
                    }
                }
                self.seen_http.push(SeenRequest {
                    account_id: params["account_id"].as_str().unwrap().to_owned(),
                    revision: params["credential_revision"].as_u64().unwrap(),
                    method: params["request"]["method"].as_str().unwrap().to_owned(),
                    url,
                    headers: serde_json::from_value(params["request"]["headers"].clone()).unwrap(),
                    body_len: payload.len(),
                    body: payload.to_vec(),
                });
                let id = format!("stream-{}", self.seen_http.len());
                self.streams.insert(id.clone(), reply.body);
                Ok((
                    json!({"status": reply.status, "http_version": "HTTP/2.0", "headers": reply.headers, "stream": id, "trailers": []}),
                    Vec::new(),
                ))
            }
            "host.http.stream_read" => {
                let id = params["stream"].as_str().unwrap();
                let max = usize::try_from(params["maximum_bytes"].as_u64().unwrap()).unwrap();
                let buf = self.streams.get_mut(id).expect("unknown stream");
                if buf.is_empty() {
                    return Ok((json!({"eof": true}), Vec::new()));
                }
                // 小块交付，确认跨块拼接
                let n = buf.len().min(max).min(40);
                let chunk: Vec<u8> = buf.drain(..n).collect();
                Ok((json!({"eof": false}), chunk))
            }
            "host.http.stream_close" => {
                self.streams.remove(params["stream"].as_str().unwrap());
                Ok((json!({}), Vec::new()))
            }
            "host.log" => {
                self.check_no_leaks(&params.to_string(), "log");
                Ok((json!({"recorded": true}), Vec::new()))
            }
            _ => panic!("unexpected host callback {method}"),
        }
    }
}

pub struct Peer {
    reader: ReadHalf<DuplexStream>,
    writer: WriteHalf<DuplexStream>,
    task: JoinHandle<()>,
    next_id: u64,
    budgets: ResolvedCallBudgets,
    callbacks: BTreeMap<u64, u64>,
}

impl Peer {
    pub async fn start(configuration: Value) -> Self {
        let (host, transport) = tokio::io::duplex(4 * 1024 * 1024);
        let (reader, writer) = tokio::io::split(transport);
        let task = tokio::spawn(async move {
            serve(reader, writer).await.unwrap();
        });
        let (reader, writer) = tokio::io::split(host);
        let mut peer = Self {
            reader,
            writer,
            task,
            next_id: 0,
            budgets: ResolvedCallBudgets::resolve(&Default::default()).unwrap(),
            callbacks: BTreeMap::new(),
        };
        peer.send(Frame::control(Message::Hello {
            handshake: Handshake {
                protocol_version: PROTOCOL_VERSION,
                artifact_sha256: "a".repeat(64),
                plugin_id: PLUGIN_ID.to_owned(),
                instance_id: "test-instance".to_owned(),
                generation: 1,
                incarnation: "test-session".to_owned(),
                configuration,
                contributes: manifest().unwrap().contributes,
            },
        }))
        .await;
        let Message::Ready { call_budgets, .. } = peer.receive().await.message else {
            panic!("handshake did not complete");
        };
        peer.budgets = ResolvedCallBudgets::resolve(&call_budgets).unwrap();
        peer
    }

    pub async fn call(
        &mut self,
        method: &str,
        stage: Stage,
        params: Value,
        payload: Vec<u8>,
        host: &mut FakeHost,
    ) -> Frame {
        self.next_id += 1;
        let id = self.next_id * 2 - 1;
        let budget = self.budgets.budget(stage);
        assert!(
            budget <= Duration::from_secs(120),
            "CPR HTTP rejects request limits above 120s: {budget:?}"
        );
        self.send(Frame {
            message: Message::Call {
                id,
                method: method.to_owned(),
                context: CallContext {
                    request: None,
                    call_id: id,
                    instance_id: "test-instance".to_owned(),
                    generation: 1,
                    incarnation: "test-session".to_owned(),
                    stage,
                    timeout_ms: u64::try_from(self.budgets.budget(stage).as_millis()).unwrap(),
                    resource_stream: false,
                    resource_scope_id: "test-scope".to_owned(),
                    request_id: None,
                    attempt_id: None,
                    account_id: None,
                    credential_revision: None,
                },
                params,
            },
            payload,
        })
        .await;
        loop {
            let frame = self.receive().await;
            match frame.message {
                Message::Callback {
                    id: callback_id,
                    parent_id,
                    method,
                    params,
                } => {
                    assert_eq!(parent_id, id);
                    let response = match host.handle(&method, &params, &frame.payload) {
                        Ok((result, payload)) => Frame {
                            message: Message::Result {
                                id: callback_id,
                                result,
                            },
                            payload,
                        },
                        Err(error) => Frame::control(Message::Error {
                            id: callback_id,
                            error,
                        }),
                    };
                    self.send(response).await;
                }
                Message::Result { id: reply_id, .. } | Message::Error { id: reply_id, .. } => {
                    assert_eq!(reply_id, id);
                    return frame;
                }
                _ => panic!("unexpected plugin message: {frame:?}"),
            }
        }
    }

    /// 管理接口；返回状态码和 JSON 正文
    pub async fn api(
        &mut self,
        method: &str,
        path: &str,
        query: &str,
        data: Option<Value>,
        host: &mut FakeHost,
    ) -> (u64, Value) {
        let params = json!({
            "method": method, "path": path, "query": query,
            "content_type": data.as_ref().map(|_| "application/json"),
        });
        let payload = data.map_or_else(Vec::new, |v| serde_json::to_vec(&v).unwrap());
        let frame = self
            .call(
                "management.handle",
                Stage::Management,
                params,
                payload,
                host,
            )
            .await;
        let Message::Result { result, .. } = &frame.message else {
            panic!("management call failed: {frame:?}");
        };
        (
            result["status"].as_u64().unwrap(),
            serde_json::from_slice(&frame.payload).unwrap(),
        )
    }

    async fn send(&mut self, frame: Frame) {
        write_frame(&mut self.writer, &frame).await.unwrap();
    }

    async fn receive(&mut self) -> Frame {
        loop {
            let frame = tokio::time::timeout(Duration::from_secs(10), read_frame(&mut self.reader))
                .await
                .expect("plugin timed out")
                .unwrap();
            match frame.message {
                Message::Suspend { .. } => {}
                Message::Resume { id, epoch } => {
                    self.send(Frame::control(Message::Resumed { id, epoch }))
                        .await;
                }
                Message::CallbackConsumed { id, parent_id } => {
                    assert_eq!(self.callbacks.remove(&id), Some(parent_id));
                }
                Message::Callback { id, parent_id, .. } => {
                    assert!(self.callbacks.insert(id, parent_id).is_none());
                    return frame;
                }
                _ => return frame,
            }
        }
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

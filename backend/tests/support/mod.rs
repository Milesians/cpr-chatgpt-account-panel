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
pub struct SeenRequest {
    pub account_id: String,
    pub revision: u64,
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, Vec<u8>)>,
    pub body_len: usize,
    pub body: Vec<u8>,
}

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
pub struct FakeHost {
    pub state: HashMap<(String, String), (Value, u64)>,
    pub accounts: Vec<Value>,
    pub http: VecDeque<HttpReply>,
    pub seen_http: Vec<SeenRequest>,
    pub calls: Vec<String>,
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

impl FakeHost {
    pub fn new(accounts: &[&str]) -> Self {
        Self {
            accounts: accounts.iter().map(|id| account(id)).collect(),
            ..Self::default()
        }
    }

    pub fn get(&self, namespace: &str, key: &str) -> Option<&Value> {
        self.state
            .get(&(namespace.to_owned(), key.to_owned()))
            .map(|(v, _)| v)
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
                let body = json!({
                    "account_id": a["account_id"], "provider_id": "openai", "credential_revision": a["credential_revision"],
                    "facts": {
                        "name": a["name"], "authentication_kind": "oauth",
                        "material": {"access_token": TOKEN, "refresh_token": "secret-refresh-token"},
                        "email": a["email"], "upstream_user_id": null, "upstream_account_id": a["upstream_account_id"],
                        "plan_type": a["plan_type"], "has_refresh_token": true,
                        "access_token_expires_at_ms": a["access_token_expires_at_ms"], "next_refresh_at_ms": null,
                    },
                });
                Ok((json!({}), serde_json::to_vec(&body).unwrap()))
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
                assert!(!text.contains(TOKEN), "a credential reached private state");
                self.next_version += 1;
                self.state
                    .insert(key, (params["value"].clone(), self.next_version));
                Ok((json!({"version": self.next_version}), Vec::new()))
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
            "host.http.account" => {
                let reply = self
                    .http
                    .pop_front()
                    .expect("unscripted account HTTP request");
                self.seen_http.push(SeenRequest {
                    account_id: params["account_id"].as_str().unwrap().to_owned(),
                    revision: params["credential_revision"].as_u64().unwrap(),
                    method: params["request"]["method"].as_str().unwrap().to_owned(),
                    url: params["request"]["url"].as_str().unwrap().to_owned(),
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
                assert!(
                    !params.to_string().contains(TOKEN),
                    "a credential reached the log"
                );
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

//! CPR 管理接口：读账号绑定的代理，调 `oauth/start` 和 `oauth/complete` 导入新账号
//!
//! 插件 SDK 的 `host.auth.save` 不能给新建账号绑定出站代理，新账号要经 CPR 管理接口的
//! `oauth/start`（带 `outboundProxyId`）+ `oauth/complete` 让 CPR 自己用那个代理换码建号。
//!
//! 管理接口经 `host.http.dispatch` 发给宿主自己的路由：只传相对路径，宿主给子请求注入插件身份，
//! 不需要管理 API Key，也不出本机网络。

use gateway_plugin_sdk::{
    call::middleware::{MiddlewareHeader, http::Version},
    client::{HostClient, HttpBody, HttpFrame, HttpRequest},
};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};

use crate::chatgpt::encode;

const MAX_BODY: usize = 8 * 1024 * 1024;
const TIMEOUT_MS: u64 = 20_000;

const ACCOUNT_DETAIL: &str = "/api/admin/accounts/detail";
const OAUTH_START: &str = "/api/admin/accounts/oauth/start";
const OAUTH_COMPLETE: &str = "/api/admin/accounts/oauth/complete";

/// 账号绑定的已保存代理（`outboundProxyId`）；代理停用时仍保留 ID，没绑或用的是内联代理地址时为空
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AccountBinding {
    #[serde(default)]
    outbound_proxy_id: Option<String>,
}

impl AccountBinding {
    fn proxy_id(self) -> Option<String> {
        self.outbound_proxy_id
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
    }
}

#[derive(Deserialize)]
struct AccountDetail {
    account: AccountBinding,
}

/// `POST /api/admin/accounts/oauth/start` 的响应数据
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthStartData {
    pub flow_id: String,
    pub authorization_url: String,
}

/// `POST /api/admin/accounts/oauth/complete` 的响应数据
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthCompleteData {
    pub account_id: String,
}

#[derive(Deserialize)]
struct Envelope {
    #[serde(default)]
    code: Value,
    #[serde(default)]
    message: String,
    #[serde(default)]
    data: Value,
}

fn one_line(s: &str, max_chars: usize) -> String {
    let collapsed: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.chars().take(max_chars).collect()
}

pub struct Admin<'a> {
    host: &'a HostClient,
}

impl<'a> Admin<'a> {
    #[must_use]
    pub const fn new(host: &'a HostClient) -> Self {
        Self { host }
    }

    async fn call<T: DeserializeOwned>(
        &self,
        method: &str,
        path: &str,
        query: &str,
        body: Option<Value>,
    ) -> Result<T, String> {
        let mut headers = vec![MiddlewareHeader {
            name: "accept".to_owned(),
            value: b"application/json".to_vec(),
        }];
        let body = match body {
            Some(body) => {
                headers.push(MiddlewareHeader {
                    name: "content-type".to_owned(),
                    value: b"application/json".to_vec(),
                });
                HttpBody::from_bytes(
                    serde_json::to_vec(&body).map_err(|_| "请求编码失败".to_owned())?,
                )
            }
            None => HttpBody::empty(),
        };
        let uri = if query.is_empty() {
            path.to_owned()
        } else {
            format!("{path}?{query}")
        };
        let mut response = self
            .host
            .dispatch_http(HttpRequest {
                settings: Value::Null,
                method: method.to_owned(),
                uri,
                version: Version::Http11,
                headers,
                timeout_ms: Some(TIMEOUT_MS),
                body,
            })
            .await
            .map_err(|_| "调不通 CPR 管理接口，请确认 CPR 版本支持插件调用管理接口".to_owned())?;
        let status = response.status;
        let mut bytes = Vec::new();
        loop {
            match response.body.read().await {
                Ok(Some(HttpFrame::Data(chunk))) => {
                    if bytes.len() + chunk.len() > MAX_BODY {
                        let _ = response.body.close().await;
                        return Err("CPR 管理接口的响应太大".into());
                    }
                    bytes.extend(chunk);
                }
                Ok(Some(_)) => {}
                Ok(None) => break,
                Err(_) => return Err("读取 CPR 管理接口的响应失败".into()),
            }
        }
        if matches!(status, 401 | 403) {
            return Err("CPR 拒绝了插件调用管理接口（没有权限）".into());
        }
        let envelope: Envelope = serde_json::from_slice(&bytes)
            .map_err(|_| format!("CPR 管理接口返回了无效响应（HTTP {status}）"))?;
        // 成功时业务码是 200（创建时 HTTP 201）
        if !(200..300).contains(&status) || envelope.code.as_u64() != Some(200) {
            return Err(format!(
                "CPR 管理接口拒绝了请求（HTTP {status}）：{}",
                one_line(&envelope.message, 200)
            ));
        }
        serde_json::from_value(envelope.data)
            .map_err(|_| "CPR 管理接口的响应格式和预期不同，可能是 CPR 版本不兼容".to_owned())
    }

    /// 账号绑定的已保存代理 ID（`GET /api/admin/accounts/detail`，只读本地记录）
    ///
    /// # Errors
    /// 管理接口失败时返回原因
    pub async fn account_proxy_id(&self, account_id: &str) -> Result<Option<String>, String> {
        let query = format!("accountId={}", encode(account_id));
        let detail: AccountDetail = self.call("GET", ACCOUNT_DETAIL, &query, None).await?;
        Ok(detail.account.proxy_id())
    }

    /// 发起新建 OpenAI OAuth 账号流程（不带 `accountId`，带 `outboundProxyId`，`name` 为邮箱）
    ///
    /// # Errors
    /// 管理接口失败时返回原因
    pub async fn oauth_start(
        &self,
        email: &str,
        outbound_proxy_id: &str,
    ) -> Result<OAuthStartData, String> {
        self.call(
            "POST",
            OAUTH_START,
            "",
            Some(json!({
                "provider": "openai",
                "name": email,
                "outboundProxyId": outbound_proxy_id,
            })),
        )
        .await
    }

    /// 提交回调链接，由 CPR 自己用已绑定的代理完成换码并建好账号
    ///
    /// # Errors
    /// 管理接口失败时返回原因
    pub async fn oauth_complete(
        &self,
        flow_id: &str,
        callback_url: &str,
    ) -> Result<OAuthCompleteData, String> {
        self.call(
            "POST",
            OAUTH_COMPLETE,
            "",
            Some(json!({
                "provider": "openai",
                "flowId": flow_id,
                "callbackUrl": callback_url,
            })),
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bound(v: Value) -> Option<String> {
        serde_json::from_value::<AccountDetail>(v)
            .unwrap()
            .account
            .proxy_id()
    }

    #[test]
    fn detail_reads_bound_proxy_id() {
        assert_eq!(
            bound(json!({"account": {"id": "a", "outboundProxyId": " p1 "}})),
            Some("p1".into())
        );
        assert_eq!(bound(json!({"account": {"outboundProxyId": "  "}})), None);
        assert_eq!(bound(json!({"account": {"outboundProxyId": null}})), None);
        assert_eq!(
            bound(json!({"account": {"outboundProxyEndpoint": "http://1.2.3.4:8080"}})),
            None
        );
    }
}

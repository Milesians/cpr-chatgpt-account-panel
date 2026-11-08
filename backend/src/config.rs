//! 插件设置（CPR 插件详情里的“设置”），握手时读一次
//!
//! 用量和账单只发 GET、不消耗额度，没有设置项；登录模块需要的设置加在这里。
//! v0.1.3 及以前的 `cprAdminUrl`、`cprAdminApiKey` 已不再使用（管理接口改经宿主内部调用），
//! 清单里仍声明它们，旧安装升级时 CPR 才不会因为多出字段拒绝；这里忽略它们。

use serde::Deserialize;
use serde_json::Value;

#[derive(Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Config {
    /// OAuth 回调地址（留空时默认 `http://localhost:1455/auth/callback`）
    pub oauth_redirect: String,
}

impl Config {
    /// 读宿主握手里的设置；字段缺失时取默认值，类型不对时整份退回默认
    #[must_use]
    pub fn from_handshake(value: &Value) -> Self {
        serde_json::from_value(value.clone()).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn ignores_retired_admin_fields() {
        let c = Config::from_handshake(&json!({
            "cprAdminUrl": "http://127.0.0.1:8080",
            "cprAdminApiKey": "old-key",
            "oauthRedirect": "http://localhost:1455/auth/callback",
        }));
        assert_eq!(c.oauth_redirect, "http://localhost:1455/auth/callback");
    }
}

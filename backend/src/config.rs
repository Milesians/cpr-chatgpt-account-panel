//! 插件设置（CPR 插件详情里的“设置”），握手时读一次
//!
//! 用量和账单只发 GET、不消耗额度，目前没有设置项；登录模块需要的设置加在这里

use serde::Deserialize;
use serde_json::Value;

#[derive(Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Config {
    /// 单密钥或密钥环配置（敏感字段，不输出到日志或调试信息）
    pub encryption_key: String,
    /// 多密钥环配置（`kid1:base64key1,kid2:base64key2`）
    pub encryption_keys: String,
    /// 当前用于加密新凭据的密钥 ID（单密钥时可留空）
    pub active_key: String,
    /// OAuth 回调地址（留空时默认 `http://localhost:1455/auth/callback`）
    pub oauth_redirect: String,
}

impl Config {
    /// 读宿主握手里的设置；字段缺失时取默认值，类型不对时整份退回默认
    #[must_use]
    pub fn from_handshake(value: &Value) -> Self {
        serde_json::from_value(value.clone()).unwrap_or_default()
    }

    /// 合并 `encryptionKeys` 与 `encryptionKey` 的原始密钥配置
    #[must_use]
    pub fn raw_encryption_keys(&self) -> &str {
        if !self.encryption_keys.trim().is_empty() {
            self.encryption_keys.trim()
        } else {
            self.encryption_key.trim()
        }
    }
}

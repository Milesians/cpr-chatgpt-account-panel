//! 插件设置（CPR 插件详情里的“设置”），握手时读一次
//!
//! 用量和账单只发 GET、不消耗额度，目前没有设置项；登录模块需要的设置加在这里

use serde::Deserialize;
use serde_json::Value;

#[derive(Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Config {}

impl Config {
    /// 读宿主握手里的设置；字段缺失时取默认值，类型不对时整份退回默认
    #[must_use]
    pub fn from_handshake(value: &Value) -> Self {
        serde_json::from_value(value.clone()).unwrap_or_default()
    }
}

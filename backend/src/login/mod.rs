//! 登录：用登录凭据（邮箱、密码、2FA）换令牌并写回 CPR，以及快速导入新账号
//!
//! 这个模块单独维护（见 README “模块分工”），这里只是和面板其余部分的接口：
//!
//! - [`routes`]：本模块的管理路由，路径都以 `api/login/` 开头，由 `management` 合进注册
//! - [`handle`]：`api/login/` 开头的请求交给它，返回 JSON 或 [`ApiError`]
//! - [`Login`]：进程内状态，插件启动时用 [`Config`] 建一次
//!
//! 需要的设置项加在 `config::Config` 和 `plugin.json` 的 `configurationSchema`（敏感的进
//! `secretFields`），私有状态的命名空间加在 `plugin.json` 的 `state`

use gateway_plugin_sdk::{
    call::management::{ManagementRequest, ManagementRoute},
    client::TypedCall,
};
use serde_json::Value;

use crate::api::ApiError;
use crate::config::Config;

/// 本模块路由的前缀
pub const PREFIX: &str = "api/login/";

/// 登录模块的进程内状态
pub struct Login {}

impl Login {
    #[must_use]
    pub fn new(_config: &Config) -> Self {
        Self {}
    }
}

/// 本模块的管理路由（`api::route("POST", "api/login/…")`）
#[must_use]
pub fn routes() -> Vec<ManagementRoute> {
    Vec::new()
}

/// 处理 `api/login/` 开头的请求
///
/// # Errors
/// 请求无效或处理失败时返回错误
pub async fn handle(
    _login: &Login,
    _call: &TypedCall<ManagementRequest>,
) -> Result<Value, ApiError> {
    Err(ApiError::new(404, "not_found", "没有这个接口"))
}

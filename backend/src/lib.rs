//! CPR 插件“ChatGPT 账号面板”：按账号查看 ChatGPT 官方用量（按使用位置，重点看 Work）、
//! 订阅与账单、登录设备。经账号在 CPR 绑定的代理，除了管理员点了的登出设备只发 GET；
//! 不刷新令牌、不改 CPR 里的账号
//!
//! 登录（用登录凭据换令牌、快速导入）在 `login` 模块，单独维护

pub mod api;
pub mod billing;
pub mod chatgpt;
pub mod clock;
pub mod config;
pub mod devices;
mod host;
pub mod login;
mod management;
pub mod store;
pub mod usage;

use std::sync::Arc;

pub use session::serve;
mod session;

use gateway_plugin_sdk::{
    Manifest, ManifestError,
    client::{AuthorError, ComposedPlugin, PluginBuilder},
};

pub const PLUGIN_ID: &str = "milesians.cpr-chatgpt-account-panel";
const MANIFEST: &[u8] = include_bytes!("../../plugin.json");

/// 打包器和运行注册共用的作者清单
///
/// # Errors
/// 清单无效时返回错误
pub fn manifest() -> Result<Manifest, ManifestError> {
    Manifest::from_author_slice(MANIFEST)
}

/// 插件进程内共享的状态；只活在这个会话里
pub struct App {
    pub config: config::Config,
    pub login: login::Login,
}

/// 组装处理器
///
/// # Errors
/// 清单与处理器不一致时返回错误
pub fn plugin(config: config::Config) -> Result<ComposedPlugin, AuthorError> {
    let app = Arc::new(App {
        login: login::Login::new(&config),
        config,
    });
    PluginBuilder::from_json(MANIFEST)?
        .management(management::registration(), move |call| {
            let app = Arc::clone(&app);
            async move { management::handle(&app, call).await }
        })?
        .build()
}

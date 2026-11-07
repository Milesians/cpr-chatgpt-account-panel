use crate::{PLUGIN_ID, config::Config, manifest, plugin};
use gateway_plugin_sdk::client::{PluginSession, SessionConfig, SessionError};

/// 生产进程和 RPC 测试共用入口，沿用 SDK 默认的 120 秒阶段预算
///
/// # Errors
/// 握手、清单、协议或传输失败时返回错误
pub async fn serve<R, W>(reader: R, writer: W) -> Result<(), Box<dyn std::error::Error>>
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
    W: tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let session = PluginSession::accept(reader, writer, SessionConfig::default()).await?;
    let manifest = manifest()?;
    if session.handshake().plugin_id != PLUGIN_ID
        || session.handshake().contributes != manifest.contributes
    {
        return Err(SessionError::Handshake.into());
    }
    let config = Config::from_handshake(&session.handshake().configuration);
    session.run(plugin(config)?).await?;
    Ok(())
}

//! 插件私有状态：每个账号最近一次官方用量查询的摘要、最近一笔支付
//!
//! 只存查询结果，不存令牌、发票链接或代理信息。键是 CPR 的账号 ID

use gateway_plugin_sdk::{ErrorCode, PluginFault, client::HostClient};
use serde::{Serialize, de::DeserializeOwned};

use crate::billing::LastPayment;
use crate::host;
use crate::usage::Summary;

pub const USAGE: &str = "usage";
pub const PAYMENTS: &str = "payments";

async fn get<T: DeserializeOwned>(
    host: &HostClient,
    namespace: &str,
    key: &str,
) -> Result<Option<(T, u64)>, PluginFault> {
    Ok(host::state_get(host, namespace, key)
        .await?
        .and_then(|r| Some((serde_json::from_value(r.value).ok()?, r.version))))
}

/// 覆盖写；并发写冲突时以后写的为准
async fn put<T: Serialize>(
    host: &HostClient,
    namespace: &str,
    key: &str,
    value: &T,
) -> Result<(), PluginFault> {
    let value = serde_json::to_value(value)
        .map_err(|_| PluginFault::new(ErrorCode::Fault, "状态编码失败"))?;
    for _ in 0..3 {
        let version = host::state_get(host, namespace, key)
            .await?
            .map(|r| r.version);
        match host::state_put(host, namespace, key, value.clone(), version).await {
            Ok(_) => return Ok(()),
            Err(e) if e.code == ErrorCode::Conflict => {}
            Err(e) => return Err(e),
        }
    }
    Err(PluginFault::new(ErrorCode::Conflict, "状态一直在被修改"))
}

/// # Errors
/// 回调失败时返回错误
pub async fn usage(host: &HostClient, account_id: &str) -> Result<Option<Summary>, PluginFault> {
    Ok(get(host, USAGE, account_id).await?.map(|(v, _)| v))
}

/// # Errors
/// 回调失败或超出配额时返回错误
pub async fn save_usage(
    host: &HostClient,
    account_id: &str,
    summary: &Summary,
) -> Result<(), PluginFault> {
    put(host, USAGE, account_id, summary).await
}

/// # Errors
/// 回调失败时返回错误
pub async fn payment(
    host: &HostClient,
    account_id: &str,
) -> Result<Option<LastPayment>, PluginFault> {
    Ok(get(host, PAYMENTS, account_id).await?.map(|(v, _)| v))
}

/// # Errors
/// 回调失败或超出配额时返回错误
pub async fn save_payment(
    host: &HostClient,
    account_id: &str,
    payment: &LastPayment,
) -> Result<(), PluginFault> {
    put(host, PAYMENTS, account_id, payment).await
}

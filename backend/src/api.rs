//! 管理接口共用的错误与响应编码（登录模块也用）

use gateway_plugin_sdk::{
    ErrorCode, PluginFault,
    call::management::{ManagementRequest, ManagementResponse, ManagementRoute},
    client::{TypedCall, TypedReply},
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::json;
use std::collections::HashMap;

use crate::chatgpt::Problem;

pub const JSON: &str = "application/json";
pub const MAX_BODY: usize = 64 * 1024;

/// 一条管理路由；非 GET 收 JSON
#[must_use]
pub fn route(method: &str, path: &str) -> ManagementRoute {
    ManagementRoute {
        method: method.to_owned(),
        path: path.to_owned(),
        request_content_types: if method == "GET" {
            Vec::new()
        } else {
            vec![JSON.to_owned()]
        },
        response_content_types: vec![JSON.to_owned()],
    }
}

#[derive(Debug)]
pub struct ApiError {
    pub status: u16,
    pub code: String,
    pub message: String,
    /// 给页面的附加信息（如可选的工作区），放在错误对象的 `details` 里
    pub details: Option<serde_json::Value>,
}

impl ApiError {
    #[must_use]
    pub fn new(status: u16, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            status,
            code: code.into(),
            message: message.into(),
            details: None,
        }
    }

    #[must_use]
    pub fn with_details(mut self, details: serde_json::Value) -> Self {
        self.details = Some(details);
        self
    }

    #[must_use]
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(400, "invalid_request", message)
    }
}

impl From<PluginFault> for ApiError {
    fn from(error: PluginFault) -> Self {
        match error.code {
            ErrorCode::Conflict => Self::new(409, "conflict", "记录刚被修改过，请重试"),
            ErrorCode::Rejected => Self::new(404, "not_found", "账号不存在"),
            _ => Self::new(502, "host_callback", "宿主操作未完成，请稍后重试"),
        }
    }
}

impl From<Problem> for ApiError {
    fn from(p: Problem) -> Self {
        let status = match p.code {
            "not_found" => 404,
            "invalid_request" | "invalid_range" | "invalid_cursor" => 400,
            "unsupported_account"
            | "account_unavailable"
            | "credential_state"
            | "token_expired"
            | "no_credentials"
            | "device_not_found"
            | "device_in_use"
            | "device_signed_out"
            | "in_use_unknown"
            | "devices_changed" => 409,
            _ => 502,
        };
        Self::new(status, p.code, p.message)
    }
}

pub type Reply = TypedReply<ManagementResponse>;

/// # Errors
/// 编码失败时返回错误
pub fn reply(status: u16, body: &impl Serialize) -> Result<Reply, PluginFault> {
    let payload =
        serde_json::to_vec(body).map_err(|_| PluginFault::new(ErrorCode::Fault, "响应编码失败"))?;
    Ok(TypedReply::new(ManagementResponse {
        headers: Vec::new(),
        status,
        content_type: JSON.to_owned(),
    })
    .with_payload(payload))
}

/// # Errors
/// 编码失败时返回错误
pub fn error_reply(e: &ApiError) -> Result<Reply, PluginFault> {
    let mut error = json!({"code": e.code, "message": e.message});
    if let Some(details) = &e.details {
        error["details"] = details.clone();
    }
    reply(e.status, &json!({ "error": error }))
}

/// 解码 JSON 请求正文
///
/// # Errors
/// 正文太大或不是预期的 JSON 时返回错误
pub fn decode<T: DeserializeOwned>(call: &TypedCall<ManagementRequest>) -> Result<T, ApiError> {
    if call.payload.len() > MAX_BODY {
        return Err(ApiError::invalid("请求正文太大"));
    }
    serde_json::from_slice(&call.payload).map_err(|_| ApiError::invalid("请求正文无效"))
}

#[must_use]
pub fn parse_query(query: &str) -> HashMap<String, String> {
    query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .map(|(k, v)| (percent_decode(k), percent_decode(v)))
        .collect()
}

/// # Errors
/// 缺少参数时返回错误
pub fn required<'a>(query: &'a HashMap<String, String>, name: &str) -> Result<&'a str, ApiError> {
    query
        .get(name)
        .map(String::as_str)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| ApiError::invalid(format!("缺少参数 {name}")))
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3])
                    .ok()
                    .and_then(|h| u8::from_str_radix(h, 16).ok());
                if let Some(b) = hex {
                    out.push(b);
                    i += 3;
                    continue;
                }
                out.push(b'%');
            }
            b'+' => out.push(b' '),
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_query() {
        let q = parse_query("id=acct_1&cursor=a%2Bb%3D&x=%zz&y=%4");
        assert_eq!(q["id"], "acct_1");
        assert_eq!(q["cursor"], "a+b=");
        assert_eq!(q["x"], "%zz");
        assert_eq!(q["y"], "%4");
        assert_eq!(parse_query("t=%41")["t"], "A");
    }
}

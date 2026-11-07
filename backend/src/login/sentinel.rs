//! 浏览器身份画像（`Identity`）与 OpenAI Sentinel PoW 令牌计算

use std::fs::File;
use std::io::Read;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::clock::now_ms;

const STD_ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const URL_ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// 标准 Base64 编码（带 `=` 填充）
#[must_use]
pub fn base64_std_encode(input: &[u8]) -> String {
    base64_encode_with(input, STD_ALPHABET, true)
}

/// URL 安全 Base64 编码（无 `=` 填充）
#[must_use]
pub fn base64_url_encode(input: &[u8]) -> String {
    base64_encode_with(input, URL_ALPHABET, false)
}

fn base64_encode_with(input: &[u8], alphabet: &[u8; 64], pad: bool) -> String {
    let mut out = Vec::with_capacity(input.len().div_ceil(3) * 4);
    let (chunks, rem) = input.as_chunks::<3>();
    for chunk in chunks {
        let n = (u32::from(chunk[0]) << 16) | (u32::from(chunk[1]) << 8) | u32::from(chunk[2]);
        out.push(alphabet[((n >> 18) & 0x3f) as usize]);
        out.push(alphabet[((n >> 12) & 0x3f) as usize]);
        out.push(alphabet[((n >> 6) & 0x3f) as usize]);
        out.push(alphabet[(n & 0x3f) as usize]);
    }
    match rem.len() {
        1 => {
            let n = u32::from(rem[0]) << 16;
            out.push(alphabet[((n >> 18) & 0x3f) as usize]);
            out.push(alphabet[((n >> 12) & 0x3f) as usize]);
            if pad {
                out.push(b'=');
                out.push(b'=');
            }
        }
        2 => {
            let n = (u32::from(rem[0]) << 16) | (u32::from(rem[1]) << 8);
            out.push(alphabet[((n >> 18) & 0x3f) as usize]);
            out.push(alphabet[((n >> 12) & 0x3f) as usize]);
            out.push(alphabet[((n >> 6) & 0x3f) as usize]);
            if pad {
                out.push(b'=');
            }
        }
        _ => {}
    }
    String::from_utf8(out).unwrap_or_default()
}

/// 兼容标准与 URL 安全 Base64 解码（自动处理有无 `=` 填充）
///
/// # Errors
/// 非法 Base64 字符或长度时返回错误
pub fn base64_decode(input: &str) -> Result<Vec<u8>, &'static str> {
    let trimmed = input.trim().trim_end_matches('=');
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    if trimmed.len() % 4 == 1 {
        return Err("invalid base64 length");
    }
    let mut out = Vec::with_capacity(trimmed.len() * 3 / 4);
    let mut buf = 0u32;
    let mut bits = 0u32;
    for b in trimmed.bytes() {
        let val = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return Err("invalid base64 character"),
        };
        buf = (buf << 6) | u32::from(val);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((buf >> bits) & 0xff) as u8);
        }
    }
    Ok(out)
}

/// 从操作系统熵源读取随机字节
///
/// # Errors
/// 读取 `/dev/urandom` 失败时返回错误
pub fn fill_random_bytes(dest: &mut [u8]) -> Result<(), &'static str> {
    let mut f = File::open("/dev/urandom").map_err(|_| "random failed")?;
    f.read_exact(dest).map_err(|_| "random failed")
}

/// 生成符合 RFC 4122 的随机 UUID v4
///
/// # Errors
/// 熵源不可用时返回错误
pub fn random_uuid_v4() -> Result<String, &'static str> {
    let mut b = [0u8; 16];
    fill_random_bytes(&mut b)?;
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    Ok(format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0],
        b[1],
        b[2],
        b[3],
        b[4],
        b[5],
        b[6],
        b[7],
        b[8],
        b[9],
        b[10],
        b[11],
        b[12],
        b[13],
        b[14],
        b[15]
    ))
}

pub const DEFAULT_SENTINEL_URL: &str = "https://sentinel.openai.com/backend-api/sentinel/req";
pub const DEFAULT_SENTINEL_SDK: &str = "https://sentinel.openai.com/sentinel/sdk.js";
pub const DEFAULT_SENTINEL_ORIGIN: &str = "https://sentinel.openai.com";
pub const DEFAULT_SENTINEL_REFERER: &str =
    "https://sentinel.openai.com/backend-api/sentinel/frame.html";
const MAX_POW_ATTEMPTS: usize = 500_000;

/// 账号绑定的模拟浏览器身份（持久化在私有状态中，不含敏感凭据）
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Identity {
    pub profile: String,
    pub device_id: String,
}

#[derive(Debug, Clone, Copy)]
pub struct BrowserProfile {
    pub id: &'static str,
    pub user_agent: &'static str,
    pub accept_language: &'static str,
    pub sec_ch_ua: &'static str,
    pub sec_ch_ua_full_version: &'static str,
    pub sec_ch_ua_mobile: &'static str,
    pub sec_ch_ua_platform: &'static str,
    pub sec_ch_ua_platform_version: &'static str,
    pub sec_ch_ua_arch: &'static str,
    pub sec_ch_ua_bitness: &'static str,
}

pub const PROFILES: [BrowserProfile; 3] = [
    BrowserProfile {
        id: "chrome-windows",
        user_agent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/136.0.0.0 Safari/537.36",
        accept_language: "en-US,en;q=0.9",
        sec_ch_ua: "\"Chromium\";v=\"136\", \"Google Chrome\";v=\"136\", \"Not.A/Brand\";v=\"99\"",
        sec_ch_ua_full_version: "\"136.0.0.0\"",
        sec_ch_ua_mobile: "?0",
        sec_ch_ua_platform: "\"Windows\"",
        sec_ch_ua_platform_version: "\"15.0.0\"",
        sec_ch_ua_arch: "\"x86\"",
        sec_ch_ua_bitness: "\"64\"",
    },
    BrowserProfile {
        id: "chrome-macos",
        user_agent: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/136.0.0.0 Safari/537.36",
        accept_language: "en-US,en;q=0.9",
        sec_ch_ua: "\"Chromium\";v=\"136\", \"Google Chrome\";v=\"136\", \"Not.A/Brand\";v=\"99\"",
        sec_ch_ua_full_version: "\"136.0.0.0\"",
        sec_ch_ua_mobile: "?0",
        sec_ch_ua_platform: "\"macOS\"",
        sec_ch_ua_platform_version: "\"14.5.0\"",
        sec_ch_ua_arch: "\"arm\"",
        sec_ch_ua_bitness: "\"64\"",
    },
    BrowserProfile {
        id: "chrome-linux",
        user_agent: "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/136.0.0.0 Safari/537.36",
        accept_language: "en-US,en;q=0.9",
        sec_ch_ua: "\"Chromium\";v=\"136\", \"Google Chrome\";v=\"136\", \"Not.A/Brand\";v=\"99\"",
        sec_ch_ua_full_version: "\"136.0.0.0\"",
        sec_ch_ua_mobile: "?0",
        sec_ch_ua_platform: "\"Linux\"",
        sec_ch_ua_platform_version: "\"6.8.0\"",
        sec_ch_ua_arch: "\"x86\"",
        sec_ch_ua_bitness: "\"64\"",
    },
];

impl BrowserProfile {
    #[must_use]
    pub fn find(id: &str) -> Option<Self> {
        let trimmed = id.trim();
        PROFILES.into_iter().find(|p| p.id == trimmed)
    }

    #[must_use]
    pub fn headers(&self) -> Vec<(String, Vec<u8>)> {
        vec![
            ("User-Agent".into(), self.user_agent.as_bytes().to_vec()),
            (
                "Accept-Language".into(),
                self.accept_language.as_bytes().to_vec(),
            ),
            ("Sec-Ch-Ua".into(), self.sec_ch_ua.as_bytes().to_vec()),
            (
                "Sec-Ch-Ua-Full-Version".into(),
                self.sec_ch_ua_full_version.as_bytes().to_vec(),
            ),
            (
                "Sec-Ch-Ua-Mobile".into(),
                self.sec_ch_ua_mobile.as_bytes().to_vec(),
            ),
            (
                "Sec-Ch-Ua-Platform".into(),
                self.sec_ch_ua_platform.as_bytes().to_vec(),
            ),
            (
                "Sec-Ch-Ua-Platform-Version".into(),
                self.sec_ch_ua_platform_version.as_bytes().to_vec(),
            ),
            (
                "Sec-Ch-Ua-Arch".into(),
                self.sec_ch_ua_arch.as_bytes().to_vec(),
            ),
            (
                "Sec-Ch-Ua-Bitness".into(),
                self.sec_ch_ua_bitness.as_bytes().to_vec(),
            ),
        ]
    }
}

/// 补全并校验账号的浏览器身份（缺省时随机分配 Profile 与 UUID v4 Device ID）
#[must_use]
pub fn ensure_identity(current: &Identity) -> (Identity, BrowserProfile, bool) {
    let mut changed = false;
    let profile = if let Some(p) = BrowserProfile::find(&current.profile) {
        p
    } else {
        changed = true;
        let mut b = [0u8; 1];
        let _ = fill_random_bytes(&mut b);
        PROFILES[(b[0] as usize) % PROFILES.len()]
    };
    let device_id = if is_uuid_v4(&current.device_id) {
        current.device_id.trim().to_ascii_lowercase()
    } else {
        changed = true;
        random_uuid_v4().unwrap_or_else(|_| "00000000-0000-4000-8000-000000000000".to_owned())
    };
    (
        Identity {
            profile: profile.id.to_owned(),
            device_id,
        },
        profile,
        changed,
    )
}

#[must_use]
pub fn is_uuid_v4(s: &str) -> bool {
    let b = s.trim().as_bytes();
    if b.len() != 36 {
        return false;
    }
    for (i, &ch) in b.iter().enumerate() {
        match i {
            8 | 13 | 18 | 23 => {
                if ch != b'-' {
                    return false;
                }
            }
            14 => {
                if ch != b'4' {
                    return false;
                }
            }
            19 => {
                if !matches!(ch, b'8' | b'9' | b'a' | b'b' | b'A' | b'B') {
                    return false;
                }
            }
            _ => {
                if !ch.is_ascii_hexdigit() {
                    return false;
                }
            }
        }
    }
    true
}

#[derive(Debug, Deserialize)]
pub struct SentinelChallenge {
    #[serde(default)]
    pub token: String,
    #[serde(default)]
    pub proofofwork: SentinelPow,
}

#[derive(Debug, Default, Deserialize)]
pub struct SentinelPow {
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub seed: String,
    #[serde(default)]
    pub difficulty: String,
}

#[must_use]
pub fn requirements_token(profile: &BrowserProfile, device_id: &str) -> String {
    let cfg = sentinel_config(profile, device_id, 1, 10, now_ms());
    let raw = serde_json::to_vec(&cfg).unwrap_or_default();
    format!("gAAAAAC{}", base64_std_encode(&raw))
}

#[must_use]
pub fn build_sentinel_token(
    profile: &BrowserProfile,
    device_id: &str,
    flow: &str,
    challenge: &SentinelChallenge,
) -> Option<String> {
    if challenge.token.trim().is_empty() {
        return None;
    }
    let proof = if challenge.proofofwork.required && !challenge.proofofwork.seed.is_empty() {
        solve_sentinel(
            profile,
            device_id,
            &challenge.proofofwork.seed,
            &challenge.proofofwork.difficulty,
        )?
    } else {
        requirements_token(profile, device_id)
    };
    let telemetry = sentinel_telemetry()?;
    let header = json!({
        "p": proof,
        "t": telemetry,
        "c": challenge.token,
        "id": device_id,
        "flow": flow,
    });
    serde_json::to_string(&header).ok()
}

#[must_use]
pub fn solve_sentinel(
    profile: &BrowserProfile,
    device_id: &str,
    seed: &str,
    difficulty: &str,
) -> Option<String> {
    let start_ms = now_ms();
    let diff = difficulty.trim().to_ascii_lowercase();
    let mut cfg = sentinel_config(profile, device_id, 0, 1, start_ms);
    for nonce in 0..MAX_POW_ATTEMPTS {
        let elapsed = (now_ms() - start_ms).max(1);
        if let Value::Array(ref mut arr) = cfg {
            arr[3] = json!(nonce);
            arr[9] = json!(elapsed);
        }
        let raw = serde_json::to_vec(&cfg).ok()?;
        let data = base64_std_encode(&raw);
        let hash = sentinel_hash(&format!("{seed}{data}"));
        if hash.len() >= diff.len() && hash[..diff.len()] <= *diff {
            return Some(format!("gAAAAAB{data}~S"));
        }
    }
    None
}

#[must_use]
pub fn sentinel_hash(value: &str) -> String {
    let mut x: u32 = 2_166_136_261;
    for &b in value.as_bytes() {
        x ^= u32::from(b);
        x = x.wrapping_mul(16_777_619);
    }
    x ^= x >> 16;
    x = x.wrapping_mul(2_246_822_507);
    x ^= x >> 13;
    x = x.wrapping_mul(3_266_489_909);
    x ^= x >> 16;
    format!("{x:08x}")
}

fn sentinel_telemetry() -> Option<String> {
    let mut raw = vec![0u8; 3500];
    fill_random_bytes(&mut raw).ok()?;
    let encoded = base64_std_encode(&raw);
    if encoded.len() < 2 {
        return None;
    }
    Some(format!("Th{}", &encoded[2..]))
}

fn sentinel_config(
    profile: &BrowserProfile,
    device_id: &str,
    nonce: usize,
    elapsed_ms: i64,
    now_unix_milli: i64,
) -> Value {
    json!([
        "1920x1080",
        format_js_utc_date(now_unix_milli),
        4_294_705_152_i64,
        nonce,
        profile.user_agent,
        DEFAULT_SENTINEL_SDK,
        Value::Null,
        Value::Null,
        "en-US",
        elapsed_ms,
        0,
        "hardwareConcurrency\u{2212}undefined",
        "location",
        "Object",
        0,
        device_id,
        "",
        8,
        now_unix_milli
    ])
}

fn format_js_utc_date(ms: i64) -> String {
    const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let total_secs = (ms / 1000).max(0);
    let days = total_secs.div_euclid(86_400);
    let day_secs = total_secs.rem_euclid(86_400);
    let hour = day_secs / 3600;
    let minute = (day_secs % 3600) / 60;
    let second = day_secs % 60;
    let wday = ((days + 4).rem_euclid(7)) as usize;

    // Howard Hinnant civil_from_days
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    let month_idx = (m - 1).clamp(0, 11) as usize;

    format!(
        "{} {} {:02} {:04} {:02}:{:02}:{:02} GMT+0000 (Coordinated Universal Time)",
        WEEKDAYS[wday], MONTHS[month_idx], d, year, hour, minute, second
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_sentinel_hash_and_pow() {
        let h = sentinel_hash("test-seed-data");
        assert_eq!(h.len(), 8);
        let (_, profile, _) = ensure_identity(&Identity::default());
        let proof = solve_sentinel(
            &profile,
            "11111111-2222-4333-8444-555555555555",
            "seed123",
            "f",
        )
        .unwrap();
        assert!(proof.starts_with("gAAAAAB"));
        assert!(proof.ends_with("~S"));
    }

    #[test]
    fn formats_utc_date_and_validates_uuid() {
        assert_eq!(
            format_js_utc_date(1_700_000_000_000),
            "Tue Nov 14 2023 22:13:20 GMT+0000 (Coordinated Universal Time)"
        );
        assert!(is_uuid_v4("11111111-2222-4333-8444-555555555555"));
        assert!(!is_uuid_v4("11111111-2222-1333-8444-555555555555"));
    }
}

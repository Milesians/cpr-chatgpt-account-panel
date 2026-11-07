//! TOTP（RFC 6238 / RFC 4226）解析与动态码计算
//!
//! 兼容裸 Base32 密钥与 `otpauth://totp/...` 链接。

use sha2::{Digest, Sha256, Sha512};

use crate::api::parse_query;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TotpAlgorithm {
    Sha1,
    Sha256,
    Sha512,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TotpSpec {
    pub secret: Vec<u8>,
    pub digits: u32,
    pub period_seconds: i64,
    pub algorithm: TotpAlgorithm,
}

/// 校验并归一化登录用的 TOTP 密钥（OpenAI MFA 要求 6 位 / 30 秒 / SHA1）
///
/// # Errors
/// 密钥为空、Base32 非法或 `otpauth://` 参数不是 6位/30秒/SHA1 时返回错误
pub fn validate_totp_secret(raw: &str) -> Result<String, &'static str> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("totp_secret_missing");
    }
    let spec = parse_totp_spec(trimmed)?;
    if spec.digits != 6 || spec.period_seconds != 30 || spec.algorithm != TotpAlgorithm::Sha1 {
        return Err("totp_invalid");
    }
    Ok(trimmed.to_owned())
}

/// 计算指定毫秒时间戳下的 TOTP 动态码
///
/// # Errors
/// 密钥为空或格式非法时返回错误码（`totp_secret_missing` 或 `totp_invalid`）
pub fn totp_code(raw: &str, now_ms: i64) -> Result<String, &'static str> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("totp_secret_missing");
    }
    let spec = parse_totp_spec(trimmed)?;
    if spec.digits != 6 || spec.period_seconds != 30 || spec.algorithm != TotpAlgorithm::Sha1 {
        return Err("totp_invalid");
    }
    generate_code(&spec, now_ms / 1000)
}

/// 解析 Base32 或 `otpauth://totp/` 链接
///
/// # Errors
/// 格式非法时返回 `"totp_invalid"`
pub fn parse_totp_spec(raw: &str) -> Result<TotpSpec, &'static str> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("totp_secret_missing");
    }
    if trimmed
        .get(..10)
        .is_some_and(|p| p.eq_ignore_ascii_case("otpauth://"))
    {
        let rest = &trimmed[10..];
        let (host_and_path, query_str) = rest.split_once('?').unwrap_or((rest, ""));
        let host = host_and_path.split('/').next().unwrap_or_default();
        if !host.eq_ignore_ascii_case("totp") {
            return Err("totp_invalid");
        }
        let query = parse_query(query_str);
        let secret_str = query.get("secret").map(String::as_str).unwrap_or_default();
        let secret = decode_base32(secret_str)?;
        let digits = match query.get("digits").map(|s| s.trim()) {
            None | Some("") => 6,
            Some("6") => 6,
            Some("8") => 8,
            _ => return Err("totp_invalid"),
        };
        let period_seconds = match query.get("period").map(|s| s.trim()) {
            None | Some("") => 30,
            Some(v) => {
                let p = v.parse::<i64>().map_err(|_| "totp_invalid")?;
                if p <= 0 {
                    return Err("totp_invalid");
                }
                p
            }
        };
        let algorithm = match query
            .get("algorithm")
            .map(|s| s.trim().to_ascii_uppercase())
            .as_deref()
        {
            None | Some("") | Some("SHA1") => TotpAlgorithm::Sha1,
            Some("SHA256") => TotpAlgorithm::Sha256,
            Some("SHA512") => TotpAlgorithm::Sha512,
            _ => return Err("totp_invalid"),
        };
        return Ok(TotpSpec {
            secret,
            digits,
            period_seconds,
            algorithm,
        });
    }
    let secret = decode_base32(trimmed)?;
    Ok(TotpSpec {
        secret,
        digits: 6,
        period_seconds: 30,
        algorithm: TotpAlgorithm::Sha1,
    })
}

pub fn generate_code(spec: &TotpSpec, unix_seconds: i64) -> Result<String, &'static str> {
    if spec.secret.is_empty() || spec.period_seconds <= 0 || !matches!(spec.digits, 6 | 8) {
        return Err("totp_invalid");
    }
    let counter = u64::try_from(unix_seconds.max(0) / spec.period_seconds)
        .map_err(|_| "totp_invalid")?
        .to_be_bytes();
    let mac = match spec.algorithm {
        TotpAlgorithm::Sha1 => hmac_sha1(&spec.secret, &counter).to_vec(),
        TotpAlgorithm::Sha256 => hmac_sha256(&spec.secret, &counter).to_vec(),
        TotpAlgorithm::Sha512 => hmac_sha512(&spec.secret, &counter).to_vec(),
    };
    let offset = (mac[mac.len() - 1] & 0x0f) as usize;
    let binary = (u32::from(mac[offset] & 0x7f) << 24)
        | (u32::from(mac[offset + 1]) << 16)
        | (u32::from(mac[offset + 2]) << 8)
        | u32::from(mac[offset + 3]);
    let modulo = 10u32.pow(spec.digits);
    let value = binary % modulo;
    Ok(format!("{value:0width$}", width = spec.digits as usize))
}

fn decode_base32(raw: &str) -> Result<Vec<u8>, &'static str> {
    let cleaned: Vec<u8> = raw
        .bytes()
        .filter(|b| !b.is_ascii_whitespace() && *b != b'-')
        .map(|b| b.to_ascii_uppercase())
        .collect();
    let unpadded = cleaned
        .iter()
        .copied()
        .take_while(|b| *b != b'=')
        .collect::<Vec<_>>();
    if unpadded.is_empty() {
        return Err("totp_invalid");
    }
    if cleaned[unpadded.len()..].iter().any(|b| *b != b'=') {
        return Err("totp_invalid");
    }
    let mut out = Vec::with_capacity(unpadded.len() * 5 / 8);
    let mut buf = 0u32;
    let mut bits = 0u32;
    for b in unpadded {
        let val = match b {
            b'A'..=b'Z' => b - b'A',
            b'2'..=b'7' => b - b'2' + 26,
            _ => return Err("totp_invalid"),
        };
        buf = (buf << 5) | u32::from(val);
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push(((buf >> bits) & 0xff) as u8);
        }
    }
    if out.is_empty() {
        return Err("totp_invalid");
    }
    Ok(out)
}

fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h0: u32 = 0x6745_2301;
    let mut h1: u32 = 0xefcd_ab89;
    let mut h2: u32 = 0x98ba_dcfe;
    let mut h3: u32 = 0x1032_5476;
    let mut h4: u32 = 0xc3d2_e1f0;

    let bit_len = (data.len() as u64) * 8;
    let mut msg = data.to_vec();
    msg.push(0x80);
    while (msg.len() % 64) != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in msg.as_chunks::<64>().0 {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[4 * i],
                chunk[4 * i + 1],
                chunk[4 * i + 2],
                chunk[4 * i + 3],
            ]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let mut a = h0;
        let mut b = h1;
        let mut c = h2;
        let mut d = h3;
        let mut e = h4;

        for (i, &wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5a82_7999),
                20..=39 => (b ^ c ^ d, 0x6ed9_eba1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8f1b_bcdc),
                _ => (b ^ c ^ d, 0xca62_c1d6),
            };
            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }

        h0 = h0.wrapping_add(a);
        h1 = h1.wrapping_add(b);
        h2 = h2.wrapping_add(c);
        h3 = h3.wrapping_add(d);
        h4 = h4.wrapping_add(e);
    }

    let mut out = [0u8; 20];
    out[0..4].copy_from_slice(&h0.to_be_bytes());
    out[4..8].copy_from_slice(&h1.to_be_bytes());
    out[8..12].copy_from_slice(&h2.to_be_bytes());
    out[12..16].copy_from_slice(&h3.to_be_bytes());
    out[16..20].copy_from_slice(&h4.to_be_bytes());
    out
}

fn hmac_sha1(key: &[u8], message: &[u8]) -> [u8; 20] {
    let mut k_block = [0u8; 64];
    if key.len() > 64 {
        let hashed = sha1(key);
        k_block[..20].copy_from_slice(&hashed);
    } else {
        k_block[..key.len()].copy_from_slice(key);
    }
    let mut ipad = [0x36u8; 64];
    let mut opad = [0x5cu8; 64];
    for i in 0..64 {
        ipad[i] ^= k_block[i];
        opad[i] ^= k_block[i];
    }
    let mut inner = Vec::with_capacity(64 + message.len());
    inner.extend_from_slice(&ipad);
    inner.extend_from_slice(message);
    let inner_hash = sha1(&inner);

    let mut outer = Vec::with_capacity(64 + 20);
    outer.extend_from_slice(&opad);
    outer.extend_from_slice(&inner_hash);
    sha1(&outer)
}

fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    let mut k_block = [0u8; 64];
    if key.len() > 64 {
        let hashed = Sha256::digest(key);
        k_block[..32].copy_from_slice(&hashed);
    } else {
        k_block[..key.len()].copy_from_slice(key);
    }
    let mut ipad = [0x36u8; 64];
    let mut opad = [0x5cu8; 64];
    for i in 0..64 {
        ipad[i] ^= k_block[i];
        opad[i] ^= k_block[i];
    }
    let mut inner = Sha256::new();
    inner.update(ipad);
    inner.update(message);
    let inner_hash = inner.finalize();

    let mut outer = Sha256::new();
    outer.update(opad);
    outer.update(inner_hash);
    outer.finalize().into()
}

fn hmac_sha512(key: &[u8], message: &[u8]) -> [u8; 64] {
    let mut k_block = [0u8; 128];
    if key.len() > 128 {
        let hashed = Sha512::digest(key);
        k_block[..64].copy_from_slice(&hashed);
    } else {
        k_block[..key.len()].copy_from_slice(key);
    }
    let mut ipad = [0x36u8; 128];
    let mut opad = [0x5cu8; 128];
    for i in 0..128 {
        ipad[i] ^= k_block[i];
        opad[i] ^= k_block[i];
    }
    let mut inner = Sha512::new();
    inner.update(ipad);
    inner.update(message);
    let inner_hash = inner.finalize();

    let mut outer = Sha512::new();
    outer.update(opad);
    outer.update(inner_hash);
    outer.finalize().into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_rfc6238_and_otpauth_codes() {
        assert_eq!(totp_code("JBSWY3DPEHPK3PXP", 0).unwrap(), "282760");
        assert_eq!(
            totp_code(
                "otpauth://totp/OpenAI:user@example.com?secret=JBSWY3DPEHPK3PXP&issuer=OpenAI",
                59_000
            )
            .unwrap(),
            "996554"
        );
        // RFC 6238 SHA1 secret "12345678901234567890" -> Base32 GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ
        assert_eq!(
            totp_code("GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ", 59_000).unwrap(),
            "287082"
        );
    }

    #[test]
    fn rejects_invalid_or_non_standard_totp() {
        assert_eq!(totp_code("", 0), Err("totp_secret_missing"));
        assert_eq!(totp_code("not-base32-!!!", 0), Err("totp_invalid"));
        assert_eq!(
            validate_totp_secret(
                "otpauth://totp/OpenAI:u@example.com?secret=JBSWY3DPEHPK3PXP&digits=8"
            ),
            Err("totp_invalid")
        );
    }
}

//! 墙上时间（UTC 毫秒）和不依赖外部库的日期换算

use std::time::{SystemTime, UNIX_EPOCH};

pub const DAY_MS: i64 = 86_400_000;

#[must_use]
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// 公历日期到 1970-01-01 起的天数（Howard Hinnant 的算法）
#[must_use]
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = i64::from(m);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// 1970-01-01 起的天数到公历日期
#[must_use]
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = u32::try_from(doy - (153 * mp + 2) / 5 + 1).unwrap_or(1);
    let m = u32::try_from(if mp < 10 { mp + 3 } else { mp - 9 }).unwrap_or(1);
    (
        if m <= 2 {
            yoe + era * 400 + 1
        } else {
            yoe + era * 400
        },
        m,
        d,
    )
}

/// UTC 毫秒所在那天，`YYYY-MM-DD`
#[must_use]
pub fn date_of(ms: i64) -> String {
    let (y, m, d) = civil_from_days(ms.div_euclid(DAY_MS));
    format!("{y:04}-{m:02}-{d:02}")
}

/// `YYYY-MM-DD`（后面可以跟别的）那天 0 点的 UTC 天数
#[must_use]
pub fn parse_day(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 10 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let y: i64 = s.get(0..4)?.parse().ok()?;
    let m: u32 = s.get(5..7)?.parse().ok()?;
    let d: u32 = s.get(8..10)?.parse().ok()?;
    if !(1..=12).contains(&m) || d == 0 || d > days_in_month(y, m) {
        return None;
    }
    Some(days_from_civil(y, m, d))
}

fn days_in_month(y: i64, m: u32) -> u32 {
    match m {
        2 if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// RFC 3339 时间（`2026-11-01T12:34:56.789Z`、`…+08:00`）到 UTC 毫秒
#[must_use]
pub fn parse_rfc3339(s: &str) -> Option<i64> {
    let s = s.trim();
    let day = parse_day(s)?;
    let b = s.as_bytes();
    if b.len() < 19 || !matches!(b[10], b'T' | b't' | b' ') || b[13] != b':' || b[16] != b':' {
        return None;
    }
    let hh: i64 = s.get(11..13)?.parse().ok()?;
    let mm: i64 = s.get(14..16)?.parse().ok()?;
    let ss: i64 = s.get(17..19)?.parse().ok()?;
    if hh > 23 || mm > 59 || ss > 60 {
        return None;
    }
    let mut rest = &s[19..];
    let mut millis = 0i64;
    if let Some(frac) = rest.strip_prefix('.') {
        let digits = frac.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 {
            return None;
        }
        let padded = format!("{:0<3}", &frac[..digits.min(3)]);
        millis = padded.parse().ok()?;
        rest = &frac[digits..];
    }
    let offset_min = match rest {
        "Z" | "z" => 0,
        _ => {
            let sign = match rest.as_bytes().first()? {
                b'+' => 1,
                b'-' => -1,
                _ => return None,
            };
            let rest = &rest[1..];
            if rest.len() != 5 || rest.as_bytes()[2] != b':' {
                return None;
            }
            let oh: i64 = rest.get(0..2)?.parse().ok()?;
            let om: i64 = rest.get(3..5)?.parse().ok()?;
            sign * (oh * 60 + om)
        }
    };
    Some(day * DAY_MS + ((hh * 60 + mm - offset_min) * 60 + ss) * 1000 + millis)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_round_trip() {
        for z in [-1000, 0, 1, 59, 365, 20_000, 20_759] {
            let (y, m, d) = civil_from_days(z);
            assert_eq!(days_from_civil(y, m, d), z);
        }
        assert_eq!(date_of(0), "1970-01-01");
        assert_eq!(parse_day("2026-11-07"), Some(days_from_civil(2026, 11, 7)));
        assert_eq!(parse_day("2026-02-30"), None);
    }

    #[test]
    fn rfc3339() {
        assert_eq!(parse_rfc3339("1970-01-01T00:00:01Z"), Some(1000));
        assert_eq!(parse_rfc3339("1970-01-01T08:00:00+08:00"), Some(0));
        assert_eq!(parse_rfc3339("1970-01-01T00:00:00.5Z"), Some(500));
        assert_eq!(parse_rfc3339("1970-01-01T00:00:00.123456Z"), Some(123));
        assert_eq!(parse_rfc3339("1970-01-01"), None);
        assert_eq!(parse_rfc3339("1970-01-01T00:00:00"), None);
    }
}

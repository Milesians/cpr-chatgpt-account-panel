//! ChatGPT 官方的 Codex 用量：一个账号的用量记在哪些使用位置，重点是 Work
//!
//! 读 Codex 用量页和 Codex CLI 的 `/usage` 用的内部接口（路由见 openai/codex 的
//! `codex-rs/backend-client`，Apache-2.0），全部是 GET：
//!
//! - `/backend-api/wham/usage`：当前的额度窗口（按 `window_minutes` 命名，不按 primary/secondary）
//! - `/backend-api/wham/profiles/me`：每天的 Token、累计
//! - `/backend-api/wham/usage/daily-token-usage-breakdown`（Team 读
//!   `daily-workspace-user-token-usage-breakdown`）：每天按使用位置、发起方式、模型的分布
//!
//! ChatGPT 按请求方报的客户端身份记使用位置。CPR 以 Codex 的身份转发，所以记在
//! `work`、`work_*`（ChatGPT Work）下面的用量一定不是 CPR 转发的
//!
//! 每部分单独读，读不到就在 `problems` 里说明、其余照常；令牌被拒、挑战页、限流整体停止

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::chatgpt::{self, Problem, Session};
use crate::clock::{DAY_MS, date_of, now_ms, parse_day, parse_rfc3339};

/// 能查的范围（天，UTC，含今天）
pub const RANGES: [u32; 3] = [7, 14, 30];

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CodexUsage {
    pub checked_at_ms: i64,
    pub days: u32,
    pub limits: Option<Limits>,
    pub tokens: Option<Tokens>,
    pub breakdown: Option<Breakdown>,
    /// 按使用位置算出来的 Work 用量；明细没读到时为空
    pub work: Option<Work>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub problems: BTreeMap<&'static str, String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Limits {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit_reached: Option<bool>,
    /// 按窗口长度从短到长；Pro 可能只有每周窗口
    pub windows: Vec<Window>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Window {
    pub used_percent: f64,
    /// 0 表示上游没说长度
    pub window_minutes: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resets_at_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Tokens {
    pub daily: Vec<TokenDay>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lifetime: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peak_daily: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub threads: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stats_as_of_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TokenDay {
    pub date: String,
    pub tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Breakdown {
    /// `relative`（个人套餐：占额度的相对用量）或 `credits`（Team）
    pub unit: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at_ms: Option<i64>,
    pub total: f64,
    pub days: Vec<BreakdownDay>,
    pub surfaces: Vec<Share>,
    pub features: Vec<Share>,
    pub models: Vec<Share>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BreakdownDay {
    pub date: String,
    pub total: f64,
    pub surfaces: BTreeMap<String, f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Share {
    pub key: String,
    pub value: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Work {
    pub total: f64,
    /// 占这段时间全部用量的比例，0～1
    pub share: f64,
    /// 最近一天有 Work 用量的日期
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_day: Option<String>,
    pub days_with_use: u32,
    /// 各 Work 位置（`work`、`work_web`…）的用量
    pub surfaces: Vec<Share>,
}

/// 上游回答不是预期的格式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unreadable;

/// 记在 ChatGPT Work 下的使用位置
#[must_use]
pub fn is_work(surface: &str) -> bool {
    surface == "work" || surface.starts_with("work_")
}

/// 位置、来源、模型的名字只放行简单字符，其余归 `other`
#[must_use]
pub fn clean_key(k: &str) -> String {
    let k = k.trim();
    let b = k.as_bytes();
    let ok = !b.is_empty()
        && b.len() <= 64
        && b[0].is_ascii_alphanumeric()
        && b.iter()
            .all(|c| c.is_ascii_alphanumeric() || b"_.:+/ -".contains(c));
    if ok {
        k.to_ascii_lowercase()
    } else {
        "other".to_owned()
    }
}

fn or_unknown(s: &str) -> &str {
    if s.trim().is_empty() { "unknown" } else { s }
}

fn valid(v: f64) -> bool {
    v.is_finite() && v >= 0.0
}

/// 读一个账号最近 `days` 天的官方用量
///
/// # Errors
/// 账号不能查、令牌被拒、挑战页、限流或 `account_id` 对不上时返回错误
pub async fn read(session: &Session<'_>, days: u32) -> Result<CodexUsage, Problem> {
    if !RANGES.contains(&days) {
        return Err(Problem::new("invalid_range", "只能查询最近 7、14 或 30 天"));
    }
    let now = now_ms();
    let end_day = now.div_euclid(DAY_MS);
    let start_day = end_day - i64::from(days) + 1;
    let mut out = CodexUsage {
        checked_at_ms: now,
        days,
        limits: None,
        tokens: None,
        breakdown: None,
        work: None,
        problems: BTreeMap::new(),
    };

    match session.get("/backend-api/wham/usage").await {
        Ok(body) => match parse_limits(&body) {
            Ok((account_id, limits)) => {
                if account_id
                    .as_deref()
                    .is_some_and(|id| id != session.workspace)
                {
                    return Err(mismatch());
                }
                out.limits = Some(limits);
            }
            Err(Unreadable) => {
                out.problems
                    .insert("limits", "ChatGPT 返回的额度数据无法识别".into());
            }
        },
        Err(p) if p.stops_query() => return Err(p),
        Err(p) => {
            out.problems.insert("limits", part_message(&p));
        }
    }

    match session.get("/backend-api/wham/profiles/me").await {
        Ok(body) => match parse_tokens(&body, start_day, end_day) {
            Ok(t) => out.tokens = Some(t),
            Err(Unreadable) => {
                out.problems
                    .insert("tokens", "ChatGPT 返回的 Token 数据无法识别".into());
            }
        },
        Err(p) if p.stops_query() => return Err(p),
        Err(p) => {
            out.problems.insert("tokens", part_message(&p));
        }
    }

    let plan = out
        .limits
        .as_ref()
        .and_then(|l| l.plan_type.as_deref())
        .or_else(|| session.plan());
    let route = if chatgpt::is_team(plan) == Some(true) {
        "daily-workspace-user-token-usage-breakdown"
    } else {
        "daily-token-usage-breakdown"
    };
    let path = format!(
        "/backend-api/wham/usage/{route}?start_date={}&end_date={}&group_by=day",
        date_of(start_day * DAY_MS),
        date_of(end_day * DAY_MS)
    );
    match session.get(&path).await {
        Ok(body) => match parse_breakdown(&body, start_day, end_day) {
            Ok(b) => {
                out.work = Some(work_of(&b));
                out.breakdown = Some(b);
            }
            Err(Unreadable) => {
                out.problems
                    .insert("breakdown", "ChatGPT 返回的用量明细无法识别".into());
            }
        },
        Err(p) if p.stops_query() => return Err(p),
        Err(p) => {
            out.problems.insert("breakdown", part_message(&p));
        }
    }
    Ok(out)
}

fn mismatch() -> Problem {
    Problem::new("account_mismatch", "用量响应与当前账号不一致，已拒绝使用")
}

fn part_message(p: &Problem) -> String {
    if p.code == "upstream_unavailable" {
        "ChatGPT 暂未提供这部分数据（接口可能已变化）".into()
    } else {
        p.message.clone()
    }
}

#[derive(Deserialize)]
struct WireWindow {
    used_percent: Option<f64>,
    limit_window_seconds: Option<i64>,
    reset_at: Option<i64>,
}

impl WireWindow {
    fn window(&self) -> Option<Window> {
        let used = self.used_percent.filter(|v| v.is_finite())?;
        Some(Window {
            used_percent: used.clamp(0.0, 100.0),
            window_minutes: self
                .limit_window_seconds
                .filter(|s| *s > 0)
                .and_then(|s| u32::try_from(s / 60).ok())
                .unwrap_or(0),
            resets_at_ms: self
                .reset_at
                .filter(|s| *s > 0 && *s < 1 << 40)
                .map(|s| s * 1000),
        })
    }
}

fn plan_name(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let ok = !b.is_empty()
        && b.len() <= 80
        && b[0].is_ascii_alphabetic()
        && b.iter()
            .all(|c| c.is_ascii_alphanumeric() || b"_. -".contains(c));
    ok.then(|| s.to_owned())
}

/// `wham/usage` → (回答里的 `account_id`, 额度)
///
/// # Errors
/// 不是预期的 JSON 时返回错误
pub fn parse_limits(body: &[u8]) -> Result<(Option<String>, Limits), Unreadable> {
    #[derive(Deserialize)]
    struct Rate {
        allowed: Option<bool>,
        limit_reached: Option<bool>,
        primary_window: Option<WireWindow>,
        secondary_window: Option<WireWindow>,
    }
    #[derive(Deserialize)]
    struct Wire {
        account_id: Option<String>,
        #[serde(default)]
        plan_type: String,
        rate_limit: Option<Rate>,
    }
    let w: Wire = serde_json::from_slice(body).map_err(|_| Unreadable)?;
    let mut limits = Limits {
        plan_type: plan_name(&w.plan_type),
        allowed: None,
        limit_reached: None,
        windows: Vec::new(),
    };
    if let Some(r) = w.rate_limit {
        limits.allowed = r.allowed;
        limits.limit_reached = r.limit_reached;
        limits.windows = [r.primary_window, r.secondary_window]
            .iter()
            .flatten()
            .filter_map(WireWindow::window)
            .collect();
        limits.windows.sort_by_key(|w| {
            if w.window_minutes == 0 {
                u32::MAX
            } else {
                w.window_minutes
            }
        });
    }
    Ok((w.account_id.filter(|s| !s.is_empty()), limits))
}

/// `wham/profiles/me` → 每天的 Token（范围内每天一项，没有用量记 0）
///
/// # Errors
/// 不是预期的 JSON 时返回错误
pub fn parse_tokens(body: &[u8], start_day: i64, end_day: i64) -> Result<Tokens, Unreadable> {
    #[derive(Deserialize)]
    struct Bucket {
        #[serde(default)]
        start_date: String,
        #[serde(default)]
        tokens: i64,
    }
    #[derive(Deserialize, Default)]
    struct Stats {
        lifetime_tokens: Option<i64>,
        peak_daily_tokens: Option<i64>,
        total_threads: Option<i64>,
        #[serde(default)]
        daily_usage_buckets: Vec<Bucket>,
    }
    #[derive(Deserialize)]
    struct Meta {
        #[serde(default)]
        stats_as_of: String,
    }
    #[derive(Deserialize)]
    struct Wire {
        metadata: Option<Meta>,
        #[serde(default)]
        stats: Stats,
    }
    let w: Wire = serde_json::from_slice(body).map_err(|_| Unreadable)?;
    let positive = |v: Option<i64>| v.and_then(|v| u64::try_from(v).ok());
    let mut by_day: HashMap<i64, u64> = HashMap::new();
    for b in &w.stats.daily_usage_buckets {
        if let (Some(d), Ok(t)) = (parse_day(&b.start_date), u64::try_from(b.tokens))
            && (start_day..=end_day).contains(&d)
        {
            *by_day.entry(d).or_default() += t;
        }
    }
    Ok(Tokens {
        daily: (start_day..=end_day)
            .map(|d| TokenDay {
                date: date_of(d * DAY_MS),
                tokens: by_day.get(&d).copied().unwrap_or(0),
            })
            .collect(),
        lifetime: positive(w.stats.lifetime_tokens),
        peak_daily: positive(w.stats.peak_daily_tokens),
        threads: positive(w.stats.total_threads),
        stats_as_of_ms: w.metadata.and_then(|m| parse_rfc3339(&m.stats_as_of)),
    })
}

/// 用量明细 → 每天按位置、整段按位置/发起方式/模型。和 Codex CLI 的读法一样：
/// 范围内每天都带 `attribution` 时以它为准，否则用位置表（和模型的 credits）
///
/// # Errors
/// 不是预期的 JSON、日期或数值不对时返回错误
pub fn parse_breakdown(body: &[u8], start_day: i64, end_day: i64) -> Result<Breakdown, Unreadable> {
    #[derive(Deserialize)]
    struct Attribution {
        #[serde(default)]
        thread_source: String,
        #[serde(default)]
        model: String,
        #[serde(default)]
        surface: String,
        value: f64,
    }
    #[derive(Deserialize)]
    struct ModelUse {
        #[serde(default)]
        model: String,
        credits: Option<f64>,
    }
    #[derive(Deserialize)]
    struct Day {
        #[serde(default)]
        date: String,
        #[serde(default)]
        product_surface_usage_values: HashMap<String, f64>,
        attribution: Option<Vec<Attribution>>,
        #[serde(default)]
        models: Vec<ModelUse>,
    }
    #[derive(Deserialize)]
    struct Wire {
        #[serde(default)]
        units: String,
        #[serde(default)]
        data_freshness_ts: String,
        #[serde(default)]
        data: Vec<Day>,
    }
    let w: Wire = serde_json::from_slice(body).map_err(|_| Unreadable)?;
    let in_range = |d: i64| (start_day..=end_day).contains(&d);
    let mut days = Vec::with_capacity(w.data.len());
    for d in &w.data {
        days.push(parse_day(&d.date).ok_or(Unreadable)?);
    }
    let attributed = w
        .data
        .iter()
        .zip(&days)
        .all(|(d, day)| !in_range(*day) || d.attribution.is_some());
    let mut surfaces: HashMap<String, f64> = HashMap::new();
    let mut features: HashMap<String, f64> = HashMap::new();
    let mut models: HashMap<String, f64> = HashMap::new();
    let mut per_day: HashMap<i64, BreakdownDay> = HashMap::new();
    let add = |m: &mut HashMap<String, f64>, k: &str, v: f64| {
        *m.entry(clean_key(k)).or_default() += v;
    };
    for (d, &day) in w.data.iter().zip(&days) {
        if !in_range(day) {
            continue;
        }
        let bd = per_day.entry(day).or_insert_with(|| BreakdownDay {
            date: date_of(day * DAY_MS),
            total: 0.0,
            surfaces: BTreeMap::new(),
        });
        if attributed {
            for a in d.attribution.iter().flatten() {
                if !valid(a.value) {
                    return Err(Unreadable);
                }
                let s = clean_key(or_unknown(&a.surface));
                bd.total += a.value;
                *bd.surfaces.entry(s.clone()).or_default() += a.value;
                *surfaces.entry(s).or_default() += a.value;
                add(&mut features, or_unknown(&a.thread_source), a.value);
                add(&mut models, or_unknown(&a.model), a.value);
            }
            continue;
        }
        for (k, &v) in &d.product_surface_usage_values {
            if !valid(v) {
                return Err(Unreadable);
            }
            let s = clean_key(k);
            bd.total += v;
            *bd.surfaces.entry(s.clone()).or_default() += v;
            *surfaces.entry(s).or_default() += v;
        }
        for m in &d.models {
            if let Some(c) = m.credits.filter(|c| valid(*c)) {
                add(&mut models, or_unknown(&m.model), c);
            }
        }
    }
    let days: Vec<BreakdownDay> = (start_day..=end_day)
        .map(|d| {
            per_day.remove(&d).unwrap_or_else(|| BreakdownDay {
                date: date_of(d * DAY_MS),
                total: 0.0,
                surfaces: BTreeMap::new(),
            })
        })
        .collect();
    Ok(Breakdown {
        unit: if w.units == "credits" {
            "credits"
        } else {
            "relative"
        },
        updated_at_ms: parse_rfc3339(&w.data_freshness_ts),
        total: days.iter().map(|d| d.total).sum(),
        days,
        surfaces: shares(surfaces),
        features: shares(features),
        models: shares(models),
    })
}

/// 按值从大到小，去掉 0
fn shares(m: HashMap<String, f64>) -> Vec<Share> {
    let mut out: Vec<Share> = m
        .into_iter()
        .filter(|(_, v)| *v > 0.0)
        .map(|(key, value)| Share { key, value })
        .collect();
    out.sort_by(|a, b| b.value.total_cmp(&a.value).then_with(|| a.key.cmp(&b.key)));
    out
}

/// 明细里的 Work 用量
#[must_use]
pub fn work_of(b: &Breakdown) -> Work {
    let surfaces: Vec<Share> = b
        .surfaces
        .iter()
        .filter(|s| is_work(&s.key))
        .cloned()
        .collect();
    let total: f64 = surfaces.iter().map(|s| s.value).sum();
    let used: Vec<&BreakdownDay> = b
        .days
        .iter()
        .filter(|d| d.surfaces.iter().any(|(k, v)| is_work(k) && *v > 0.0))
        .collect();
    Work {
        total,
        share: if b.total > 0.0 { total / b.total } else { 0.0 },
        last_day: used.last().map(|d| d.date.clone()),
        days_with_use: u32::try_from(used.len()).unwrap_or(u32::MAX),
        surfaces,
    }
}

/// 列表上记下的最近一次查询结果（不含凭据）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    pub checked_at_ms: i64,
    pub days: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work: Option<Work>,
    /// 用量最多的几个位置
    #[serde(default)]
    pub surfaces: Vec<Share>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<f64>,
    /// 各窗口（分钟）用了百分之多少
    #[serde(default)]
    pub windows: Vec<(u32, f64)>,
    /// 读不到的部分
    #[serde(default)]
    pub problems: Vec<String>,
    /// 整次查询失败时的原因
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<SummaryError>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SummaryError {
    pub code: String,
    pub message: String,
}

impl Summary {
    #[must_use]
    pub fn of(u: &CodexUsage) -> Self {
        Self {
            checked_at_ms: u.checked_at_ms,
            days: u.days,
            unit: u.breakdown.as_ref().map(|b| b.unit.to_owned()),
            work: u.work.clone(),
            surfaces: u
                .breakdown
                .as_ref()
                .map(|b| b.surfaces.iter().take(8).cloned().collect())
                .unwrap_or_default(),
            total: u.breakdown.as_ref().map(|b| b.total),
            windows: u
                .limits
                .as_ref()
                .map(|l| {
                    l.windows
                        .iter()
                        .map(|w| (w.window_minutes, w.used_percent))
                        .collect()
                })
                .unwrap_or_default(),
            problems: u.problems.keys().map(|k| (*k).to_owned()).collect(),
            error: None,
        }
    }

    #[must_use]
    pub fn failed(days: u32, p: &Problem) -> Self {
        Self {
            checked_at_ms: now_ms(),
            days,
            unit: None,
            work: None,
            surfaces: Vec::new(),
            total: None,
            windows: Vec::new(),
            problems: Vec::new(),
            error: Some(SummaryError {
                code: p.code.to_owned(),
                message: p.message.clone(),
            }),
        }
    }
}

/// 给浏览器的 JSON；数值都是有限的
#[must_use]
pub fn to_value(u: &CodexUsage) -> Value {
    serde_json::to_value(u).unwrap_or(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::days_from_civil;

    fn day(d: u32) -> i64 {
        days_from_civil(2026, 11, d)
    }

    #[test]
    fn limits_sorted_by_window_length() {
        let body = br#"{"account_id":"ws","plan_type":"pro","rate_limit":{"allowed":true,
            "primary_window":{"used_percent":40,"limit_window_seconds":604800,"reset_at":1800000000},
            "secondary_window":{"used_percent":120,"limit_window_seconds":18000}}}"#;
        let (id, l) = parse_limits(body).unwrap();
        assert_eq!(id.as_deref(), Some("ws"));
        assert_eq!(l.plan_type.as_deref(), Some("pro"));
        assert_eq!(l.windows[0].window_minutes, 300);
        assert!((l.windows[0].used_percent - 100.0).abs() < f64::EPSILON);
        assert_eq!(l.windows[1].window_minutes, 10080);
        assert_eq!(l.windows[1].resets_at_ms, Some(1_800_000_000_000));
    }

    #[test]
    fn pro_with_only_weekly_window() {
        let body = br#"{"plan_type":"pro","rate_limit":{"primary_window":{"used_percent":5,"limit_window_seconds":604800},"secondary_window":null}}"#;
        let (_, l) = parse_limits(body).unwrap();
        assert_eq!(l.windows.len(), 1);
        assert_eq!(l.windows[0].window_minutes, 10080);
    }

    #[test]
    fn tokens_fill_every_day() {
        let body = br#"{"metadata":{"stats_as_of":"2026-11-07T00:00:00Z"},"stats":{"lifetime_tokens":9,
            "daily_usage_buckets":[{"start_date":"2026-11-06","tokens":5},{"start_date":"2026-10-01","tokens":7}]}}"#;
        let t = parse_tokens(body, day(5), day(7)).unwrap();
        assert_eq!(
            t.daily.iter().map(|d| d.tokens).collect::<Vec<_>>(),
            vec![0, 5, 0]
        );
        assert_eq!(t.lifetime, Some(9));
        assert!(t.stats_as_of_ms.is_some());
    }

    #[test]
    fn breakdown_by_attribution_and_work() {
        let body = br#"{"units":"relative","data":[
            {"date":"2026-11-05","attribution":[{"surface":"desktop_app","thread_source":"user","model":"gpt-5","value":3}]},
            {"date":"2026-11-06","attribution":[{"surface":"work_web","thread_source":"user","model":"gpt-5","value":1},
                                                {"surface":"Work","thread_source":"","model":"","value":1}]}]}"#;
        let b = parse_breakdown(body, day(5), day(7)).unwrap();
        assert_eq!(b.days.len(), 3);
        assert!((b.total - 5.0).abs() < 1e-9);
        assert_eq!(b.surfaces[0].key, "desktop_app");
        assert!(b.features.iter().any(|s| s.key == "unknown"));
        let w = work_of(&b);
        assert!((w.total - 2.0).abs() < 1e-9);
        assert!((w.share - 0.4).abs() < 1e-9);
        assert_eq!(w.last_day.as_deref(), Some("2026-11-06"));
        assert_eq!(w.days_with_use, 1);
        assert_eq!(w.surfaces.len(), 2);
    }

    #[test]
    fn breakdown_falls_back_to_surface_map() {
        let body = br#"{"units":"credits","data":[
            {"date":"2026-11-07","product_surface_usage_values":{"cli":2,"<script>":1},"models":[{"model":"gpt-5","credits":3}]},
            {"date":"2026-11-06","attribution":[]}]}"#;
        let b = parse_breakdown(body, day(6), day(7)).unwrap();
        assert_eq!(b.unit, "credits");
        assert!(b.surfaces.iter().any(|s| s.key == "other"));
        assert_eq!(b.models[0].key, "gpt-5");
        assert_eq!(work_of(&b).total, 0.0);
        assert_eq!(work_of(&b).last_day, None);
    }

    #[test]
    fn breakdown_rejects_bad_values() {
        assert!(parse_breakdown(br#"{"data":[{"date":"x"}]}"#, day(1), day(2)).is_err());
        assert!(
            parse_breakdown(
                br#"{"data":[{"date":"2026-11-01","attribution":[{"value":-1}]}]}"#,
                day(1),
                day(2)
            )
            .is_err()
        );
    }

    #[test]
    fn work_surfaces() {
        assert!(is_work("work"));
        assert!(is_work("work_desktop"));
        assert!(!is_work("workspace"));
        assert!(!is_work("desktop_app"));
    }
}

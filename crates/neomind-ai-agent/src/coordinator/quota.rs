//! Cross-day daily quota helpers (business day in config timezone).

use chrono::{TimeZone, Utc};
use chrono_tz::Tz;

use crate::model::{AgentStats, PacingPolicy, TriggerSpec};

/// Resolve the IANA timezone used for `max_runs_per_day`.
///
/// Priority: pacing.timezone → cron trigger timezone → UTC.
pub fn quota_timezone(pacing: &PacingPolicy, trigger: &TriggerSpec) -> String {
    if let Some(tz) = pacing.timezone.as_ref().filter(|s| !s.is_empty()) {
        return tz.clone();
    }
    if let TriggerSpec::Cron { timezone, .. } = trigger {
        if !timezone.is_empty() {
            return timezone.clone();
        }
    }
    "UTC".into()
}

/// `YYYY-MM-DD` for `now_ms` in `tz_name` (invalid TZ falls back to UTC).
pub fn business_day_key(now_ms: i64, tz_name: &str) -> String {
    let secs = now_ms.div_euclid(1000);
    let utc = Utc
        .timestamp_opt(secs, 0)
        .single()
        .unwrap_or_else(|| Utc.timestamp_opt(0, 0).single().expect("epoch"));
    match tz_name.parse::<Tz>() {
        Ok(tz) => utc.with_timezone(&tz).format("%Y-%m-%d").to_string(),
        Err(_) => utc.format("%Y-%m-%d").to_string(),
    }
}

/// Roll `runs_today` / `quota_day` forward when the business day changes.
///
/// Returns the current business-day key after any reset.
pub fn roll_quota_day(stats: &mut AgentStats, now_ms: i64, tz_name: &str) -> String {
    let day = business_day_key(now_ms, tz_name);
    if stats.quota_day.as_deref() != Some(day.as_str()) {
        stats.runs_today = 0;
        stats.quota_day = Some(day.clone());
    }
    day
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::PacingPolicy;

    #[test]
    fn shanghai_midnight_is_not_utc_midnight() {
        // 2024-01-01 16:00 UTC == 2024-01-02 00:00 Asia/Shanghai
        let ms = 1_704_124_800_000i64; // 2024-01-01T16:00:00Z
        assert_eq!(business_day_key(ms, "UTC"), "2024-01-01");
        assert_eq!(business_day_key(ms, "Asia/Shanghai"), "2024-01-02");
    }

    #[test]
    fn roll_resets_runs_on_day_change() {
        let mut stats = AgentStats {
            runs_today: 5,
            quota_day: Some("2024-01-01".into()),
            ..Default::default()
        };
        let day = roll_quota_day(&mut stats, 1_704_124_800_000, "Asia/Shanghai");
        assert_eq!(day, "2024-01-02");
        assert_eq!(stats.runs_today, 0);
        assert_eq!(stats.quota_day.as_deref(), Some("2024-01-02"));
    }

    #[test]
    fn pacing_timezone_wins_over_cron() {
        let pacing = PacingPolicy {
            timezone: Some("Asia/Shanghai".into()),
            ..Default::default()
        };
        let trigger = TriggerSpec::Cron {
            expr: "0 * * * *".into(),
            timezone: "UTC".into(),
        };
        assert_eq!(quota_timezone(&pacing, &trigger), "Asia/Shanghai");
    }
}

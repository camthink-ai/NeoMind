//! External/extension result normalization and valid-window checks.

use crate::model::{EvidenceItem, EvidenceQuality};

/// Mark stale when observed_at is older than max_age_secs relative to now.
pub fn enforce_freshness(item: &mut EvidenceItem, now_ms: i64, max_age_secs: u64) {
    if let Some(obs) = item.observed_at {
        let age_ms = now_ms.saturating_sub(obs);
        if age_ms > (max_age_secs as i64) * 1000 {
            item.quality = EvidenceQuality::Stale;
        }
    }
}

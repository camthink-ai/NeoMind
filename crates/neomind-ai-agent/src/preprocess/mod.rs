//! Preprocessors (timeseries / vision / external). M1 stubs + external normalize.

pub mod external;

use crate::model::EffectivePlan;

/// Resolve static preprocess path into EffectivePlan warnings (no I/O).
pub fn preview_preprocess_path(plan: &EffectivePlan) -> Vec<String> {
    let mut out = Vec::new();
    for r in &plan.authorized_reads {
        if r.source_id.starts_with("extension:") {
            out.push(format!("external_normalize:{}", r.key));
        }
    }
    out
}

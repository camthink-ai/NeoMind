//! Adaptive prompt-token calibration from measured `prompt_eval` counts (S3-3 / NM-294).
//!
//! Records measured vs estimated prompt tokens per backend/model and maintains
//! an EWMA correction factor clamped to `[0.65, 1.35]`. No external tokenizer
//! files and no LLM protocol changes — estimates stay heuristic, just scaled.

use std::collections::HashMap;

/// EWMA smoothing factor (higher = react faster to new observations).
pub const EWMA_ALPHA: f64 = 0.2;
/// Lower clamp — prevents under-estimation from collapsing the safety margin.
pub const FACTOR_MIN: f64 = 0.65;
/// Upper clamp — prevents a single over-count from over-inflating budgets.
pub const FACTOR_MAX: f64 = 1.35;
/// Neutral starting factor before any observation.
pub const DEFAULT_FACTOR: f64 = 1.0;

/// Build a stable calibration key for a backend/model pair.
pub fn calibration_key(backend_id: Option<&str>, model: Option<&str>) -> String {
    format!(
        "{}::{}",
        backend_id.unwrap_or("default"),
        model.unwrap_or("unknown")
    )
}

/// Update an EWMA factor from one measured/estimated observation.
///
/// Returns the previous factor (clamped) when either side is non-positive so a
/// bad meter reading cannot poison the calibration.
pub fn update_ewma_factor(prev: f64, measured: f64, estimated: f64) -> f64 {
    let prev = prev.clamp(FACTOR_MIN, FACTOR_MAX);
    if measured <= 0.0 || estimated <= 0.0 || !measured.is_finite() || !estimated.is_finite() {
        return prev;
    }
    let ratio = measured / estimated;
    if !ratio.is_finite() || ratio <= 0.0 {
        return prev;
    }
    let next = prev * (1.0 - EWMA_ALPHA) + ratio * EWMA_ALPHA;
    next.clamp(FACTOR_MIN, FACTOR_MAX)
}

/// Apply a calibration factor to a raw heuristic estimate.
pub fn apply_calibration(estimated: usize, factor: f64) -> usize {
    let f = factor.clamp(FACTOR_MIN, FACTOR_MAX);
    ((estimated as f64) * f).round().max(0.0) as usize
}

/// Scale system/tool breakdown so it never exceeds a measured total (ContextUsage consistency).
pub fn fit_breakdown_to_measured(
    system_tokens: usize,
    tool_tokens: usize,
    measured_prompt: u32,
) -> (usize, usize) {
    let measured = measured_prompt as usize;
    let sum = system_tokens.saturating_add(tool_tokens);
    if sum == 0 || measured == 0 || sum <= measured {
        return (system_tokens, tool_tokens);
    }
    let scale = measured as f64 / sum as f64;
    let sys = ((system_tokens as f64) * scale).floor() as usize;
    let tools = measured.saturating_sub(sys);
    (sys, tools)
}

/// In-memory per-backend/model EWMA factors.
#[derive(Debug, Default, Clone)]
pub struct PromptTokenCalibration {
    factors: HashMap<String, f64>,
}

impl PromptTokenCalibration {
    pub fn new() -> Self {
        Self {
            factors: HashMap::new(),
        }
    }

    pub fn factor(&self, key: &str) -> f64 {
        self.factors
            .get(key)
            .copied()
            .unwrap_or(DEFAULT_FACTOR)
            .clamp(FACTOR_MIN, FACTOR_MAX)
    }

    pub fn observe(&mut self, key: &str, measured: f64, estimated: f64) -> f64 {
        let prev = self.factor(key);
        let next = update_ewma_factor(prev, measured, estimated);
        self.factors.insert(key.to_string(), next);
        next
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_clamps_to_bounds() {
        // Huge under-estimate → ratio 2.0 but clamp at 1.35
        let f = update_ewma_factor(1.0, 2000.0, 1000.0);
        assert!(f <= FACTOR_MAX);
        assert!(f > 1.0);

        // Huge over-estimate → ratio 0.5 but clamp floor 0.65 after enough steps
        let mut f = 1.0;
        for _ in 0..40 {
            f = update_ewma_factor(f, 500.0, 1000.0);
        }
        assert!((f - FACTOR_MIN).abs() < 1e-9 || f >= FACTOR_MIN);
        assert!(f >= FACTOR_MIN);
    }

    #[test]
    fn update_ignores_non_positive_meters() {
        assert_eq!(update_ewma_factor(1.1, 0.0, 1000.0), 1.1);
        assert_eq!(update_ewma_factor(1.1, 1000.0, 0.0), 1.1);
        assert_eq!(update_ewma_factor(1.1, f64::NAN, 1000.0), 1.1);
    }

    #[test]
    fn ewma_moves_toward_ratio() {
        let mut f = 1.0;
        // measured consistently 80% of estimate → factor drifts down
        for _ in 0..20 {
            f = update_ewma_factor(f, 800.0, 1000.0);
        }
        assert!(f < 0.95, "expected drift toward 0.8, got {f}");
        assert!(f > 0.75, "should not overshoot too far after 20 steps: {f}");
    }

    #[test]
    fn apply_calibration_rounds() {
        assert_eq!(apply_calibration(1000, 1.0), 1000);
        assert_eq!(apply_calibration(1000, 0.8), 800);
        assert_eq!(apply_calibration(1000, 1.35), 1350);
        assert_eq!(apply_calibration(1000, 0.65), 650);
        // Out-of-range factor still clamps
        assert_eq!(apply_calibration(1000, 2.0), 1350);
        assert_eq!(apply_calibration(1000, 0.1), 650);
    }

    #[test]
    fn fit_breakdown_never_exceeds_measured() {
        let (s, t) = fit_breakdown_to_measured(3000, 4000, 5000);
        assert_eq!(s + t, 5000);
        assert!(s < 3000);
        let (s2, t2) = fit_breakdown_to_measured(1000, 500, 5000);
        assert_eq!((s2, t2), (1000, 500));
    }

    #[test]
    fn store_observes_per_key() {
        let mut store = PromptTokenCalibration::new();
        assert_eq!(store.factor("a::m"), DEFAULT_FACTOR);
        let f = store.observe("a::m", 800.0, 1000.0);
        assert!(f < 1.0);
        assert_eq!(store.factor("a::m"), f);
        assert_eq!(store.factor("other::m"), DEFAULT_FACTOR);
    }

    #[test]
    fn calibration_key_format() {
        assert_eq!(calibration_key(Some("b1"), Some("mini")), "b1::mini");
        assert_eq!(calibration_key(None, None), "default::unknown");
    }
}

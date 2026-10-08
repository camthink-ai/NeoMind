//! Centralized tool timeout governance.
//!
//! Before this module existed, each tool hardcoded its own timeout:
//! `extension_tools` 300s, `shell` 30s (max 600s), `vision` 10s,
//! `web_fetch` 15s. The numbers were right but they were scattered,
//! which made "raise the shell ceiling" or "tighten vision" a code
//! spelunking exercise.
//!
//! All tool timeouts SHOULD route through this module. Keeping them
//! in one place makes the ceiling/floor visible and lets us tune a
//! single source of truth when extension ML inference starts taking
//! longer or when network paths get slower.

use std::time::Duration;

/// Fast tier — I/O we expect to complete in single-digit seconds.
/// Used for vision HTTP fetch (image download before analysis).
pub const FAST: Duration = Duration::from_secs(10);

/// Default tier — interactive tools that the agent waits on
/// turn-by-turn. Used for the shell tool's default per-invocation cap.
pub const DEFAULT: Duration = Duration::from_secs(30);

/// Network tier — single-resource HTTP fetches where 10s is too tight
/// for slow mobile links but 30s invites hangs. Used by `web_fetch`.
pub const NETWORK: Duration = Duration::from_secs(15);

/// Slow tier — long-running operations like extension ML inference
/// (YOLO, OCR, etc). The shell scheduler can't interrupt these midway;
/// they need a generous ceiling.
pub const SLOW: Duration = Duration::from_secs(300);

/// Absolute ceiling — no tool may run longer than this in a single call.
/// Enforced as `min(chosen, HARD_MAX)` everywhere we apply timeouts.
pub const HARD_MAX: Duration = Duration::from_secs(600);

#[inline]
pub fn shell_default() -> Duration {
    DEFAULT
}

#[inline]
pub fn shell_max() -> Duration {
    HARD_MAX
}

#[inline]
pub fn vision_capture() -> Duration {
    FAST
}

#[inline]
pub fn web_fetch() -> Duration {
    NETWORK
}

#[inline]
pub fn extension_invoke() -> Duration {
    SLOW
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Invariant: tiers must be strictly ordered so callers can compose them.
    /// If this ever breaks, the labels stop meaning what they say.
    #[test]
    fn tier_ordering() {
        assert!(FAST < NETWORK, "FAST must be < NETWORK");
        assert!(NETWORK < DEFAULT, "NETWORK must be < DEFAULT");
        assert!(DEFAULT < SLOW, "DEFAULT must be < SLOW");
        assert!(SLOW < HARD_MAX, "SLOW must be < HARD_MAX");
    }

    /// Floor/cap contract: every accessor returns a value within [FAST, HARD_MAX].
    #[test]
    fn accessors_stay_within_bounds() {
        let lo = FAST;
        let hi = HARD_MAX;
        for v in [
            shell_default(),
            shell_max(),
            vision_capture(),
            web_fetch(),
            extension_invoke(),
        ] {
            assert!(v >= lo && v <= hi, "timeout {v:?} outside [{lo:?}, {hi:?}]");
        }
    }
}

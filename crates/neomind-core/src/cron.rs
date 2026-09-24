//! Cron expressions, in the form people actually write them.
//!
//! The `cron` crate wants seconds first — six or seven fields. Everyone else —
//! every other system, our CLI help, the rule validator's own error message,
//! and every user — writes the standard five-field form
//! (`min hour day month weekday`). So `"0 8 * * *"`, the example the CLI gives
//! and the shape the rule validator recommends, failed to parse: creating an
//! agent that runs daily at eight came back `Invalid cron expression` for
//! anyone who followed the documentation.
//!
//! [`normalize`] supplies the missing seconds field. It only ever widens: a
//! six-field expression is returned untouched and still parses, so nothing
//! already stored under the stricter rule breaks.

/// A cron expression the `cron` crate will accept.
///
/// Five fields get a `0` second prepended; anything else is returned as-is, for
/// the parser to take or reject on its own terms.
///
/// ```
/// assert_eq!(neomind_core::cron::normalize("0 8 * * *"), "0 0 8 * * *");
/// assert_eq!(neomind_core::cron::normalize("0 0 8 * * *"), "0 0 8 * * *");
/// ```
pub fn normalize(expression: &str) -> String {
    if expression.split_whitespace().count() == 5 {
        format!("0 {expression}")
    } else {
        expression.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_documented_five_field_form_becomes_six() {
        assert_eq!(normalize("0 8 * * *"), "0 0 8 * * *");
        assert_eq!(normalize("*/5 * * * *"), "0 */5 * * * *");
        // The rule validator's own example.
        assert_eq!(normalize("0 */5 * * *"), "0 0 */5 * * *");
    }

    #[test]
    fn everything_else_is_left_alone() {
        // Already six or seven fields — the crate's native shape.
        assert_eq!(normalize("0 0 8 * * *"), "0 0 8 * * *");
        assert_eq!(normalize("0 0 8 * * * 2026"), "0 0 8 * * * 2026");
        // Not five fields and not valid either; the parser says so, not us.
        assert_eq!(normalize("nonsense"), "nonsense");
        assert_eq!(normalize(""), "");
    }

    #[test]
    fn what_it_returns_actually_parses() {
        // The point of the function: hand its output to the crate.
        for expr in ["0 8 * * *", "*/5 * * * *", "30 2 * * 1-5"] {
            let normalized = normalize(expr);
            assert!(
                normalized.parse::<cron::Schedule>().is_ok(),
                "{expr} normalised to {normalized}, which still does not parse"
            );
        }
    }
}

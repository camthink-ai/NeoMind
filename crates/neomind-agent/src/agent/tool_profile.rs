//! Dynamic tool exposure profiles (S3-2 / NM-293).
//!
//! Tools stay registered at runtime; profiles only gate first-round
//! *exposure* so small context windows (≤16K / 8K MiniCPM) do not pay for
//! high-cost, low-frequency schemas such as `image_edit` and `file_edit`.

/// Tool exposure profile selected from the active model's `max_context`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolProfile {
    /// `max_context <= 16_384` — defer heavy schemas.
    Light,
    /// `16_384 < max_context < 32_768` — hide unused image_edit only.
    Balanced,
    /// `max_context >= 32_768` — standard full exposure (plus VLM/image gates).
    Standard,
}

/// Select a profile from the model's context window size.
pub fn select_tool_profile(max_context: usize) -> ToolProfile {
    if max_context <= 16_384 {
        ToolProfile::Light
    } else if max_context < 32_768 {
        ToolProfile::Balanced
    } else {
        ToolProfile::Standard
    }
}

/// Whether `tool_name` should be exposed on this turn for the given profile.
///
/// Runtime capabilities are preserved — hidden tools remain executable if the
/// model somehow names them; they are simply omitted from the request schema.
pub fn tool_exposed_in_profile(
    tool_name: &str,
    profile: ToolProfile,
    has_user_images: bool,
    vlm_available: bool,
) -> bool {
    // Vision: omit when the user already attached images (inline) OR no VLM.
    if tool_name == "vision" && (has_user_images || !vlm_available) {
        return false;
    }

    match profile {
        ToolProfile::Light => match tool_name {
            // High-cost / low-freq: defer unless the turn already has images.
            "image_edit" => has_user_images,
            // Heavy file-edit + fetch schemas: on-demand for small windows.
            "file_edit" | "web_fetch" => false,
            // `vision` stays reachable on Light when vlm_available && !has_user_images
            // (filtered above when images are already inline or no VLM is configured).
            _ => true,
        },
        ToolProfile::Balanced => {
            // Mid windows: keep Stage-1 image_edit deferral for pure-text turns.
            tool_name != "image_edit" || has_user_images
        }
        ToolProfile::Standard => {
            // Full exposure (≥32K). Vision still gated by VLM / inline images above.
            true
        }
    }
}

/// Filter a tool-definition list for the active profile / turn.
pub fn filter_tools_for_profile<T, F>(
    tools: impl IntoIterator<Item = T>,
    name_of: F,
    max_context: usize,
    has_user_images: bool,
    vlm_available: bool,
) -> Vec<T>
where
    F: Fn(&T) -> &str,
{
    let profile = select_tool_profile(max_context);
    tools
        .into_iter()
        .filter(|t| tool_exposed_in_profile(name_of(t), profile, has_user_images, vlm_available))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_light_balanced_standard_by_context() {
        assert_eq!(select_tool_profile(8_192), ToolProfile::Light);
        assert_eq!(select_tool_profile(16_384), ToolProfile::Light);
        assert_eq!(select_tool_profile(16_385), ToolProfile::Balanced);
        assert_eq!(select_tool_profile(24_000), ToolProfile::Balanced);
        assert_eq!(select_tool_profile(32_767), ToolProfile::Balanced);
        assert_eq!(select_tool_profile(32_768), ToolProfile::Standard);
        assert_eq!(select_tool_profile(131_072), ToolProfile::Standard);
    }

    #[test]
    fn light_defers_heavy_tools_without_images() {
        assert!(!tool_exposed_in_profile(
            "image_edit",
            ToolProfile::Light,
            false,
            true
        ));
        assert!(!tool_exposed_in_profile(
            "file_edit",
            ToolProfile::Light,
            false,
            true
        ));
        assert!(!tool_exposed_in_profile(
            "web_fetch",
            ToolProfile::Light,
            false,
            true
        ));
        // Pure-text Light turns with a configured VLM keep `vision` reachable
        // (e.g., analyzing camera history images), while hiding it when no VLM
        // is available or when images are already attached inline.
        assert!(tool_exposed_in_profile(
            "vision",
            ToolProfile::Light,
            false,
            true
        ));
        assert!(!tool_exposed_in_profile(
            "vision",
            ToolProfile::Light,
            false,
            false
        ));
        assert!(tool_exposed_in_profile(
            "shell",
            ToolProfile::Light,
            false,
            true
        ));
        assert!(tool_exposed_in_profile(
            "file_write",
            ToolProfile::Light,
            false,
            true
        ));
        assert!(tool_exposed_in_profile(
            "web_search",
            ToolProfile::Light,
            false,
            true
        ));
    }

    #[test]
    fn light_exposes_image_edit_when_turn_has_images() {
        assert!(tool_exposed_in_profile(
            "image_edit",
            ToolProfile::Light,
            true,
            true
        ));
        // Inline images: vision stays hidden (model already sees them).
        assert!(!tool_exposed_in_profile(
            "vision",
            ToolProfile::Light,
            true,
            true
        ));
        // file_edit / web_fetch stay deferred even with images.
        assert!(!tool_exposed_in_profile(
            "file_edit",
            ToolProfile::Light,
            true,
            true
        ));
        assert!(!tool_exposed_in_profile(
            "web_fetch",
            ToolProfile::Light,
            true,
            true
        ));
    }

    #[test]
    fn vision_hidden_without_vlm_or_when_images_inline() {
        assert!(!tool_exposed_in_profile(
            "vision",
            ToolProfile::Standard,
            false,
            false
        ));
        assert!(!tool_exposed_in_profile(
            "vision",
            ToolProfile::Light,
            true,
            true
        ));
        assert!(tool_exposed_in_profile(
            "vision",
            ToolProfile::Standard,
            false,
            true
        ));
    }

    #[test]
    fn balanced_hides_image_edit_without_images_keeps_file_edit() {
        assert!(!tool_exposed_in_profile(
            "image_edit",
            ToolProfile::Balanced,
            false,
            true
        ));
        assert!(tool_exposed_in_profile(
            "file_edit",
            ToolProfile::Balanced,
            false,
            true
        ));
    }

    #[test]
    fn filter_tools_for_profile_drops_deferred_names() {
        let names = vec![
            "shell",
            "image_edit",
            "file_edit",
            "vision",
            "web_fetch",
            "memory",
        ];
        // Light + VLM available + pure-text turn keeps `vision` reachable.
        let kept = filter_tools_for_profile(names.clone(), |s| *s, 8_192, false, true);
        assert_eq!(kept, vec!["shell", "vision", "memory"]);
        // Without a VLM backend, `vision` is omitted.
        let kept_no_vlm = filter_tools_for_profile(names.clone(), |s| *s, 8_192, false, false);
        assert_eq!(kept_no_vlm, vec!["shell", "memory"]);
        // When user images are already inline, `image_edit` is exposed and `vision` is omitted.
        let kept_inline = filter_tools_for_profile(names, |s| *s, 8_192, true, true);
        assert_eq!(kept_inline, vec!["shell", "image_edit", "memory"]);
    }
}

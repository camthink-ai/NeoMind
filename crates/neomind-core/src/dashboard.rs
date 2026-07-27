//! Dashboard built-in widget catalog.
//!
//! Single source of truth for the static set of dashboard component types
//! shipped with the frontend. Surfaced via `GET /api/frontend-components`
//! (the `builtin_types` field) so CLI callers and agents can enumerate
//! available widget types without guessing.
//!
//! When adding a new built-in component in
//! `web/src/pages/dashboard-components/Renderers.tsx` (`builtInTypes` set
//! and `builtInComponentMap`), also append its type name here. The
//! frontend registry remains authoritative for actual rendering; this
//! module only mirrors the type-name list.

/// A built-in widget type shipped with the frontend dashboard.
#[derive(Debug, Clone)]
pub struct BuiltinWidgetType {
    /// Type identifier used in dashboard component definitions
    /// (matches the keys of `builtInComponentMap` in `Renderers.tsx`).
    pub type_id: &'static str,
    /// Human-readable name.
    pub display_name: &'static str,
    /// Coarse category for grouping in listings.
    pub category: &'static str,
}

/// Catalogue of built-in widget types.
///
/// Kept in sync with `builtInTypes` in
/// `web/src/pages/dashboard-components/Renderers.tsx`. Order is grouping-
/// friendly (indicators → charts → controls → display → spatial → layout).
pub const BUILTIN_WIDGET_TYPES: &[BuiltinWidgetType] = &[
    // Indicators — single value display
    BuiltinWidgetType {
        type_id: "value-card",
        display_name: "Value Card",
        category: "indicator",
    },
    BuiltinWidgetType {
        type_id: "counter",
        display_name: "Counter",
        category: "indicator",
    },
    BuiltinWidgetType {
        type_id: "metric-card",
        display_name: "Metric Card",
        category: "indicator",
    },
    BuiltinWidgetType {
        type_id: "led-indicator",
        display_name: "LED Indicator",
        category: "indicator",
    },
    BuiltinWidgetType {
        type_id: "sparkline",
        display_name: "Sparkline",
        category: "indicator",
    },
    BuiltinWidgetType {
        type_id: "progress-bar",
        display_name: "Progress Bar",
        category: "indicator",
    },
    // Charts — time-series / categorical
    BuiltinWidgetType {
        type_id: "line-chart",
        display_name: "Line Chart",
        category: "chart",
    },
    BuiltinWidgetType {
        type_id: "area-chart",
        display_name: "Area Chart",
        category: "chart",
    },
    BuiltinWidgetType {
        type_id: "bar-chart",
        display_name: "Bar Chart",
        category: "chart",
    },
    BuiltinWidgetType {
        type_id: "pie-chart",
        display_name: "Pie Chart",
        category: "chart",
    },
    // Controls — interactive
    BuiltinWidgetType {
        type_id: "toggle-switch",
        display_name: "Toggle Switch",
        category: "control",
    },
    // Display — static content
    BuiltinWidgetType {
        type_id: "image-display",
        display_name: "Image Display",
        category: "display",
    },
    BuiltinWidgetType {
        type_id: "image-history",
        display_name: "Image History",
        category: "display",
    },
    BuiltinWidgetType {
        type_id: "web-display",
        display_name: "Web Display",
        category: "display",
    },
    BuiltinWidgetType {
        type_id: "markdown-display",
        display_name: "Markdown Display",
        category: "display",
    },
    // Spatial — geographic / video
    BuiltinWidgetType {
        type_id: "map-display",
        display_name: "Map Display",
        category: "spatial",
    },
    BuiltinWidgetType {
        type_id: "video-display",
        display_name: "Video Display",
        category: "spatial",
    },
    // Layout — escape hatch
    BuiltinWidgetType {
        type_id: "custom-layer",
        display_name: "Custom Layer",
        category: "layout",
    },
];

/// Return the list of built-in widget type identifiers (in catalogue order).
pub fn builtin_type_ids() -> Vec<&'static str> {
    BUILTIN_WIDGET_TYPES.iter().map(|t| t.type_id).collect()
}

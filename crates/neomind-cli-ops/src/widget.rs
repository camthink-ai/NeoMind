use crate::types::CliResponse;
use crate::ApiClient;
use anyhow::Result;
use serde_json::json;
use std::fs;
use std::path::Path;

/// Widget type categories for scaffolding
const WIDGET_CATEGORIES: &[&str] = &["chart", "gauge", "stat", "table", "image", "custom"];

/// Scaffold a new widget component directory with manifest.json and bundle.js IIFE template.
/// This generates files locally under the project data directory — it does NOT call the API.
/// The user (or AI) then edits the files and installs via `neomind widget install`.
pub fn create_widget(
    name: &str,
    widget_type: &str,
    output_dir: Option<&str>,
) -> Result<CliResponse> {
    // Validate widget type
    if !WIDGET_CATEGORIES.contains(&widget_type) {
        return Ok(CliResponse::error(
            format!(
                "Invalid widget type '{}'. Must be one of: {}",
                widget_type,
                WIDGET_CATEGORIES.join(", ")
            ),
            "INVALID_TYPE",
        ));
    }

    // Generate a slug-style ID from name (ASCII-only for valid JS global names)
    let widget_id: String = name
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    // Collapse multiple consecutive hyphens
    let widget_id: String = widget_id
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let widget_id = if widget_id.is_empty() {
        // Non-ASCII name: generate a fallback ID from widget type + hash
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        name.hash(&mut hasher);
        let hash = hasher.finish() % 10000;
        format!("{}-{:04}", widget_type, hash)
    } else {
        widget_id
    };

    let global_name = format!("NeoMind{}", to_pascal_case(&widget_id));

    // Determine output directory: use data/frontend-components/{widget_id} by default
    let dir_path = if let Some(custom) = output_dir {
        Path::new(custom).to_path_buf()
    } else {
        let data_dir = crate::auto_auth::data_dir_for_paths();
        Path::new(&data_dir)
            .join("frontend-components")
            .join(&widget_id)
    };

    // Create directory
    fs::create_dir_all(&dir_path)?;

    // Generate manifest.json
    let manifest = json!({
        "id": widget_id,
        "name": {
            "en": name,
            "zh": name
        },
        "description": {
            "en": format!("{} widget", name),
            "zh": format!("{}组件", name)
        },
        "icon": get_icon_for_type(widget_type),
        "category": widget_type,
        "global_name": global_name,
        "export_name": "default",
        "size_constraints": {
            "min_width": 2,
            "min_height": 2
        },
        "has_data_source": true,
        "max_data_sources": 1,
        "config_schema": {
            "type": "object",
            "properties": {}
        },
        "default_config": {}
    });

    let manifest_path = dir_path.join("manifest.json");
    fs::write(&manifest_path, serde_json::to_string_pretty(&manifest)?)?;

    // Generate bundle.js IIFE template
    let bundle_template = generate_bundle_template(&widget_id, &global_name, widget_type);
    let bundle_path = dir_path.join("bundle.js");
    fs::write(&bundle_path, bundle_template)?;

    let dir_str = dir_path.display().to_string();
    Ok(CliResponse::success(
        json!({
            "id": widget_id,
            "name": name,
            "type": widget_type,
            "directory": dir_str,
            "registered": false,
            "files": [
                { "name": "manifest.json", "path": manifest_path.display().to_string() },
                { "name": "bundle.js", "path": bundle_path.display().to_string() }
            ]
        }),
        // Scaffold ≠ deliverable: bundle.js is a TYPE TEMPLATE, and the
        // scaffold is not registered until `widget install`. Both facts
        // were missed live (round 7): the model treated the gauge template
        // as the finished component and moved straight to adding it to a
        // dashboard. State the whole pipeline in the success message.
        format!(
            "Widget scaffold created (TEMPLATE code, not yet registered): {dir_str}. \
             Next: 1) edit bundle.js to implement the component, \
             2) `neomind widget install {widget_id}` to register, \
             3) shell {{command:\"neomind dashboard add-components <dashboard-id>\", \
             params:[{{id:\"c1\",type:\"{widget_id}\",position:{{x:0,y:0,w:4,h:3}}}}]}}. \
             Shortcut: re-run create with --install to scaffold AND register in one step."
        ),
    ))
}

/// Generate IIFE bundle.js template following NeoMind-Dashboard-Components standards.
/// Uses `window.React` and `window.jsxRuntime` — no build tools needed.
fn generate_bundle_template(widget_id: &str, global_name: &str, widget_type: &str) -> String {
    let component_name = to_pascal_case(widget_id);

    // Common card shell style: border + rounded corners + card background
    let card_style = "width: '100%', height: '100%', border: '1px solid var(--color-border)', borderRadius: '0.5rem', background: 'var(--color-card)', overflow: 'hidden'";

    // Different templates based on widget type
    let component_body = match widget_type {
        "chart" => format!(
            r#"  // Chart widget — line chart over props.dataSource (array of {{value, timestamp}})
  const {{ useEffect, useRef }} = React;
  const containerRef = useRef(null);

  useEffect(() => {{
    const container = containerRef.current;
    if (!container) return;
    const data = Array.isArray(props.dataSource) ? props.dataSource : [];

    // Canvas can't use var(--*) directly — resolve design tokens per draw
    const draw = () => {{
      const canvas = container.querySelector('canvas');
      if (!canvas || container.clientWidth === 0) return;
      const css = getComputedStyle(container);
      const lineColor = css.getPropertyValue('--color-primary').trim() || '#3b82f6';
      const textColor = css.getPropertyValue('--color-text-muted').trim() || '#94a3b8';
      const gridColor = css.getPropertyValue('--color-border').trim() || '#e2e8f0';

      const dpr = window.devicePixelRatio || 1;
      const w = container.clientWidth;
      const h = container.clientHeight;
      canvas.width = w * dpr;
      canvas.height = h * dpr;
      const ctx = canvas.getContext('2d');
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.clearRect(0, 0, w, h);
      if (data.length === 0) return;

      const pts = data.map(d => Number(d && d.value)).filter(v => Number.isFinite(v));
      if (pts.length === 0) return;
      const min = Math.min.apply(null, pts);
      const max = Math.max.apply(null, pts);
      const span = (max - min) || 1;

      const pad = {{ top: 10, right: 12, bottom: 20, left: 40 }};
      const iw = Math.max(w - pad.left - pad.right, 10);
      const ih = Math.max(h - pad.top - pad.bottom, 10);
      const xAt = i => pad.left + (pts.length === 1 ? iw / 2 : (i / (pts.length - 1)) * iw);
      const yAt = v => pad.top + ih - ((v - min) / span) * ih;

      // Gridlines + value labels
      ctx.strokeStyle = gridColor;
      ctx.fillStyle = textColor;
      ctx.font = '10px sans-serif';
      ctx.textAlign = 'right';
      ctx.lineWidth = 1;
      for (let g = 0; g <= 3; g++) {{
        const gy = pad.top + (ih / 3) * g;
        const val = max - (span / 3) * g;
        ctx.beginPath();
        ctx.moveTo(pad.left, gy);
        ctx.lineTo(w - pad.right, gy);
        ctx.stroke();
        ctx.fillText(String(Math.round(val * 100) / 100), pad.left - 6, gy + 3);
      }}

      // Data line
      ctx.strokeStyle = lineColor;
      ctx.lineWidth = 2;
      ctx.beginPath();
      for (let i = 0; i < pts.length; i++) {{
        if (i === 0) ctx.moveTo(xAt(i), yAt(pts[i]));
        else ctx.lineTo(xAt(i), yAt(pts[i]));
      }}
      ctx.stroke();

      // Latest value: dot on the line + label in the top-right corner
      const lx = xAt(pts.length - 1);
      const ly = yAt(pts[pts.length - 1]);
      ctx.fillStyle = lineColor;
      ctx.beginPath();
      ctx.arc(lx, ly, 3, 0, Math.PI * 2);
      ctx.fill();
      ctx.fillStyle = textColor;
      ctx.fillText(String(pts[pts.length - 1]), w - pad.right, pad.top + 8);
    }};

    draw();
    // Redraw on container resize (dashboard grid drag/resize)
    const ro = typeof ResizeObserver !== 'undefined' ? new ResizeObserver(draw) : null;
    if (ro) ro.observe(container);
    return () => {{ if (ro) ro.disconnect(); }};
  }}, [props.dataSource]);

  return React.createElement('div', {{
    style: {{ {card_style}, position: 'relative' }},
    ref: containerRef,
  }},
    React.createElement('canvas', {{ style: {{ position: 'absolute', top: 0, left: 0, width: '100%', height: '100%' }} }}),
    (!Array.isArray(props.dataSource) || props.dataSource.length === 0) &&
      React.createElement('span', {{ style: {{ position: 'absolute', top: 0, left: 0, width: '100%', height: '100%', display: 'flex', alignItems: 'center', justifyContent: 'center', color: 'var(--color-text-muted)' }} }}, '{component_name} Chart')
  );"#
        ),
        "gauge" => format!(
            r#"  // Gauge widget — displays a single metric value
  const value = props.dataSource?.[0]?.value ?? 0;
  const max = props.config?.max ?? 100;

  return React.createElement('div', {{
    style: {{ {card_style}, display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center' }},
  }},
    React.createElement('div', {{ style: {{ fontSize: '2rem', fontWeight: 'bold', color: 'var(--color-text-primary)' }} }}, String(value)),
    React.createElement('div', {{ style: {{ color: 'var(--color-text-muted)', marginTop: '0.5rem' }} }}, '{component_name}'),
    React.createElement('div', {{ style: {{ width: '80%', height: '4px', background: 'var(--color-border)', borderRadius: '2px', marginTop: '1rem' }} }},
      React.createElement('div', {{ style: {{ width: Math.min(value / max * 100, 100) + '%', height: '100%', background: 'var(--color-success)', borderRadius: '2px' }}}})
    )
  );"#
        ),
        "stat" => format!(
            r#"  // Stat widget — displays a key metric with label
  const value = props.dataSource?.[0]?.value ?? '-';
  const label = props.config?.label ?? '{component_name}';

  return React.createElement('div', {{
    style: {{ {card_style}, display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center', padding: '1rem' }},
  }},
    React.createElement('div', {{ style: {{ fontSize: '2.5rem', fontWeight: 'bold', color: 'var(--color-text-primary)' }} }}, String(value)),
    React.createElement('div', {{ style: {{ fontSize: '0.875rem', color: 'var(--color-text-muted)', marginTop: '0.25rem' }} }}, label)
  );"#
        ),
        "table" => format!(
            r#"  // Table widget — displays data in tabular form
  const rows = props.dataSource ?? [];

  return React.createElement('div', {{
    style: {{ {card_style}, overflow: 'auto', padding: '0.5rem' }},
  }},
    React.createElement('table', {{ style: {{ width: '100%', borderCollapse: 'collapse' }} }},
      React.createElement('thead', null,
        React.createElement('tr', null,
          ['Timestamp', 'Value'].map(h =>
            React.createElement('th', {{
              key: h,
              style: {{ padding: '0.5rem', textAlign: 'left', borderBottom: '1px solid var(--color-border)', color: 'var(--color-text-muted)', fontSize: '0.75rem' }}
            }}, h)
          )
        )
      ),
      React.createElement('tbody', null,
        rows.slice(0, 10).map((row, i) =>
          React.createElement('tr', {{ key: i }},
            React.createElement('td', {{ style: {{ padding: '0.5rem', borderBottom: '1px solid var(--color-border)', fontSize: '0.875rem' }} }},
              row.timestamp ?? ''),
            React.createElement('td', {{ style: {{ padding: '0.5rem', borderBottom: '1px solid var(--color-border)', fontSize: '0.875rem' }} }},
              String(row.value ?? ''))
          )
        )
      )
    )
  );"#
        ),
        "image" => format!(
            r#"  // Image widget — displays an image from data source
  const imageUrl = props.config?.url ?? '';
  const alt = props.config?.alt ?? '{component_name}';

  return React.createElement('div', {{
    style: {{ {card_style}, display: 'flex', alignItems: 'center', justifyContent: 'center' }},
  }},
    imageUrl
      ? React.createElement('img', {{ src: imageUrl, alt, style: {{ maxWidth: '100%', maxHeight: '100%', objectFit: 'contain' }} }})
      : React.createElement('span', {{ style: {{ color: 'var(--color-text-muted)' }} }}, '{component_name} Image')
  );"#
        ),
        _ => format!(
            r#"  // Custom widget — implement your logic here
  return React.createElement('div', {{
    style: {{ {card_style}, display: 'flex', alignItems: 'center', justifyContent: 'center' }},
  }},
    React.createElement('span', {{ style: {{ color: 'var(--color-text-muted)' }} }}, '{component_name}')
  );"#
        ),
    };

    format!(
        r#"// {component_name} Widget — {widget_id}
// Generated by neomind widget create
// NeoMind Dashboard Component (IIFE format)
//
// Runtime: uses window.React and window.jsxRuntime (provided by NeoMind)
// Styling: CSS variables only (var(--color-*), var(--spacing-*), etc.)
// Container: must fill w-full h-full

(function(global) {{
  'use strict';

  var React = global.React;
  var jsxRuntime = global.jsxRuntime;

  function {component_name}(props) {{
{component_body}
  }}

  // Assign to global for NeoMind component registry
  global['{global_name}'] = {component_name};

}})(window);
"#
    )
}

fn to_pascal_case(s: &str) -> String {
    s.split('-')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect()
}

fn get_icon_for_type(widget_type: &str) -> &'static str {
    match widget_type {
        "chart" => "bar-chart-2",
        "gauge" => "gauge",
        "stat" => "activity",
        "table" => "table",
        "image" => "image",
        _ => "box",
    }
}

/// Resolve a widget `name` that may be a plain string or an i18n object
/// (`{"en":"...","zh":"..."}`). Prefer `en`, then `zh`, then any string value.
fn display_i18n_name(name: &serde_json::Value) -> String {
    if let Some(s) = name.as_str() {
        return s.to_string();
    }
    if let Some(obj) = name.as_object() {
        for key in ["en", "zh", "zh-CN", "zh_CN"] {
            if let Some(s) = obj.get(key).and_then(|v| v.as_str()) {
                if !s.is_empty() {
                    return s.to_string();
                }
            }
        }
        for v in obj.values() {
            if let Some(s) = v.as_str() {
                if !s.is_empty() {
                    return s.to_string();
                }
            }
        }
    }
    "(unnamed)".to_string()
}

/// Extract compact summary items for extension-contributed dashboard components.
fn summarize_extension_widgets(ext_data: &serde_json::Value) -> Vec<serde_json::Value> {
    let Some(items) = extract_list_array(ext_data, "components") else {
        return Vec::new();
    };
    items
        .iter()
        .map(|w| {
            let id = w
                .get("type")
                .or_else(|| w.get("id"))
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let mut entry = json!({
                "id": id,
                "name": w.get("name").map(display_i18n_name).unwrap_or_else(|| "(unnamed)".to_string()),
                "category": w.get("category").and_then(|v| v.as_str()).unwrap_or("extension"),
                "source": "extension",
                "has_data_source": w.get("has_data_source").and_then(|v| v.as_bool()).unwrap_or(false),
            });
            if let Some(ext_id) = w.get("extension_id").and_then(|v| v.as_str()) {
                entry["extension_id"] = json!(ext_id);
            }
            if let Some(desc) = w.get("description").and_then(|v| v.as_str()) {
                if !desc.is_empty() {
                    entry["description"] = json!(desc);
                }
            }
            entry
        })
        .collect()
}

/// Find a single extension-contributed dashboard component by its `type` (or `id`).
fn find_extension_widget(ext_data: &serde_json::Value, id: &str) -> Option<serde_json::Value> {
    let items = extract_list_array(ext_data, "components")?;
    items.into_iter().find(|w| {
        w.get("type").and_then(|v| v.as_str()) == Some(id)
            || w.get("id").and_then(|v| v.as_str()) == Some(id)
    })
}

/// Produce a placeholder JSON value from a JSON Schema property definition.
fn sample_value_from_schema(prop: &serde_json::Value, key: &str) -> serde_json::Value {
    if let Some(default) = prop.get("default") {
        if !default.is_null() {
            return default.clone();
        }
    }
    if let Some(first_enum) = prop
        .get("enum")
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
    {
        return first_enum.clone();
    }
    match prop.get("type").and_then(|v| v.as_str()).unwrap_or("") {
        "boolean" => json!(false),
        "integer" | "number" => json!(0),
        "array" => json!([]),
        "object" => {
            let mut obj = serde_json::Map::new();
            if let Some(sub) = prop.get("properties").and_then(|v| v.as_object()) {
                for (k, v) in sub {
                    obj.insert(k.clone(), sample_value_from_schema(v, k));
                }
            }
            serde_json::Value::Object(obj)
        }
        _ => json!(format!("<{}>", key)),
    }
}

/// Synthesize a ready-to-copy `neomind dashboard add-components` example
/// payload for ANY widget (built-in, extension-provided, marketplace, or
/// custom). Gives AI agents the exact JSON structure (`id`, `type`, `title`,
/// `position`, `data_source`, `display`, `config`) without guessing.
fn build_usage_example(
    id: &str,
    component: &serde_json::Value,
    source: &str,
) -> serde_json::Value {
    let title = component
        .get("name")
        .map(display_i18n_name)
        .filter(|s| s != "(unnamed)")
        .unwrap_or_else(|| id.to_string());

    // Size from size_constraints (built-in / community) or default_size (extension)
    let sc = component.get("size_constraints");
    let ds_size = component.get("default_size");
    let w = sc
        .and_then(|s| s.get("default_w").or_else(|| s.get("min_w")).or_else(|| s.get("min_width")))
        .or_else(|| ds_size.and_then(|s| s.get("w").or_else(|| s.get("width"))))
        .and_then(|v| v.as_u64())
        .unwrap_or(4);
    let h = sc
        .and_then(|s| s.get("default_h").or_else(|| s.get("min_h")).or_else(|| s.get("min_height")))
        .or_else(|| ds_size.and_then(|s| s.get("h").or_else(|| s.get("height"))))
        .and_then(|v| v.as_u64())
        .unwrap_or(3);

    let has_data_source = component
        .get("has_data_source")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let has_device_binding = component
        .get("has_device_binding")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let mut item = json!({
        "id": format!("{}-1", id),
        "type": id,
        "title": title,
        "position": { "x": 0, "y": 0, "w": w, "h": h }
    });

    // 1. data_source tailored by widget type & source
    let mut notes: Vec<String> = Vec::new();
    match id {
        "agent-monitor-widget" => {
            item["data_source"] = json!({
                "type": "agent",
                "agentId": "<agent-id>"
            });
            notes.push(
                "Bind the target agent in `data_source` (`type: \"agent\"`, `agentId`), NOT inside `config`. Run `neomind agent list` to get valid agent IDs.".to_string(),
            );
        }
        "ai-analyst" => {
            item["data_source"] = json!([{
                "type": "telemetry",
                "source": "device",
                "id": "<device-id>",
                "field": "<metric-name>",
                "mode": "timeseries",
                "sourceId": "<device-id>",
                "metricId": "<metric-name>",
                "timeRange": 1,
                "limit": 50,
                "timeWindow": { "type": "last_24hours" }
            }]);
            notes.push(
                "`data_source` is optional context telemetry; customize `config.systemPrompt`, `config.modelId`, or `config.contextWindowSize` (1-100).".to_string(),
            );
        }
        "toggle-switch" => {
            item["data_source"] = json!({
                "type": "command",
                "source": "device",
                "id": "<device-id>",
                "field": "<command-name>",
                "deviceId": "<device-id>",
                "command": "<command-name>"
            });
            notes.push(
                "Use `type: \"command\"` with a real device command from `neomind device get <device-id>` (or `type: \"extension-command\"` for extension commands).".to_string(),
            );
        }
        "line-chart" | "area-chart" | "bar-chart" | "sparkline" | "image-history" => {
            item["data_source"] = json!({
                "type": "telemetry",
                "source": "device",
                "id": "<device-id>",
                "field": "<metric-name>",
                "mode": "timeseries",
                "sourceId": "<device-id>",
                "metricId": "<metric-name>",
                "timeRange": 1,
                "limit": 50,
                "timeWindow": { "type": "last_24hours" }
            });
        }
        "map-display" | "custom-layer" | "video-display" | "web-display" | "markdown-display" => {
            // Configured primarily via `config` (bindings / src / content)
        }
        _ if source == "extension" && has_data_source => {
            let ext_id = component
                .get("extension_id")
                .and_then(|v| v.as_str())
                .unwrap_or("<extension-id>");
            item["data_source"] = json!({
                "type": "extension-metric",
                "source": "extension",
                "id": ext_id,
                "field": "<COMMAND>:<FIELD>",
                "mode": "timeseries",
                "extensionId": ext_id,
                "extensionMetric": "<COMMAND>:<FIELD>"
            });
            notes.push(format!(
                "Discover `<COMMAND>:<FIELD>` via `neomind extension get {}`.",
                ext_id
            ));
        }
        _ if has_data_source => {
            item["data_source"] = json!({
                "type": "telemetry",
                "source": "device",
                "id": "<device-id>",
                "field": "<metric-name>",
                "mode": "latest",
                "sourceId": "<device-id>",
                "metricId": "<metric-name>",
                "timeRange": 1,
                "limit": 50
            });
        }
        _ => {}
    }

    // 2. display & config seeded from default_config + config_schema + complex built-in templates
    let mut display_obj = serde_json::Map::new();
    let mut config_obj = serde_json::Map::new();

    if let Some(def_cfg) = component.get("default_config").and_then(|v| v.as_object()) {
        // Built-in manifests wrap defaults as {"display": {...}, "config": {...}};
        // community & extension manifests store flat config keys directly.
        let is_enveloped = def_cfg.contains_key("display") || def_cfg.contains_key("config");
        if is_enveloped {
            if let Some(d) = def_cfg.get("display").and_then(|v| v.as_object()) {
                for (k, v) in d {
                    display_obj.insert(k.clone(), v.clone());
                }
            }
            if let Some(c) = def_cfg.get("config").and_then(|v| v.as_object()) {
                for (k, v) in c {
                    config_obj.insert(k.clone(), v.clone());
                }
            }
        } else {
            for (k, v) in def_cfg {
                config_obj.insert(k.clone(), v.clone());
            }
        }
    }

    // Fill concrete examples for complex built-in widgets so AI sees exact nested shapes
    match id {
        "led-indicator" => {
            config_obj.entry("rules".to_string()).or_insert_with(|| {
                json!([
                    { "threshold": { "operator": ">", "value": 80 }, "state": "error", "label": "High" },
                    { "threshold": { "operator": ">=", "value": 0 }, "state": "on", "label": "Normal" }
                ])
            });
        }
        "map-display" => {
            config_obj.insert(
                "bindings".to_string(),
                json!([{
                    "id": "m1",
                    "type": "metric",
                    "name": "Sensor 1",
                    "dataSource": {
                        "type": "telemetry",
                        "source": "device",
                        "id": "<device-id>",
                        "field": "<metric-name>",
                        "mode": "latest",
                        "sourceId": "<device-id>",
                        "metricId": "<metric-name>"
                    },
                    "position": { "lat": 39.9042, "lng": 116.4074 }
                }]),
            );
            notes.push(
                "Add map markers inside `config.bindings` with `{lat, lng}` coordinates and a `dataSource` object.".to_string(),
            );
        }
        "custom-layer" => {
            config_obj.insert(
                "bindings".to_string(),
                json!([{
                    "id": "b1",
                    "type": "metric",
                    "name": "Temp",
                    "dataSource": {
                        "type": "telemetry",
                        "source": "device",
                        "id": "<device-id>",
                        "field": "<metric-name>",
                        "mode": "latest",
                        "sourceId": "<device-id>",
                        "metricId": "<metric-name>"
                    },
                    "position": { "x": 30, "y": 40 },
                    "fontSize": "sm"
                }]),
            );
            notes.push(
                "Add overlay elements in `config.bindings`; `position.x/y` are 0-100 percentage coordinates on the canvas.".to_string(),
            );
        }
        "video-display" => {
            config_obj
                .entry("src".to_string())
                .or_insert_with(|| json!("https://example.com/stream/index.m3u8"));
        }
        "web-display" => {
            config_obj
                .entry("src".to_string())
                .or_insert_with(|| json!("https://example.com"));
        }
        "image-display" => {
            config_obj
                .entry("src".to_string())
                .or_insert_with(|| json!("https://example.com/snapshot.jpg"));
        }
        _ => {
            // For custom / marketplace / extension widgets with a flat JSON Schema,
            // populate missing schema properties with defaults or typed placeholders.
            if source != "builtin" {
                if let Some(props) = component
                    .get("config_schema")
                    .and_then(|s| s.get("properties"))
                    .and_then(|p| p.as_object())
                {
                    for (k, prop_schema) in props {
                        config_obj
                            .entry(k.clone())
                            .or_insert_with(|| sample_value_from_schema(prop_schema, k));
                    }
                }
            }
        }
    }

    if has_device_binding {
        config_obj
            .entry("deviceBinding".to_string())
            .or_insert_with(|| json!({ "deviceId": "<device-id>" }));
        notes.push(
            "This component uses device binding (`config.deviceBinding.deviceId`). Run `neomind device list` for valid device IDs.".to_string(),
        );
    }

    if !display_obj.is_empty() {
        item["display"] = serde_json::Value::Object(display_obj);
    }
    if !config_obj.is_empty() {
        item["config"] = serde_json::Value::Object(config_obj);
    }

    let mut usage = json!({
        "command": "neomind dashboard add-components <DASHBOARD_ID>",
        "params": [item]
    });
    if !notes.is_empty() {
        usage["notes"] = json!(notes.join(" "));
    }
    usage
}

/// List all available widgets with compact summary across every source:
/// 1) installed community / marketplace / custom widgets (`/frontend-components`)
/// 2) extension-provided dashboard widgets (`/extensions/dashboard-components`)
/// 3) built-in widget types (`builtin_types` from `neomind_core::dashboard::BUILTIN_WIDGET_TYPES`)
///
/// Full manifest & `config_schema` for any widget (built-in, custom/market, or
/// extension-provided) is available via `neomind widget get <id>`.
pub async fn list_widgets(client: &ApiClient) -> Result<CliResponse> {
    let data = client.get("/frontend-components").await?;
    // Best-effort fetch of extension-provided dashboard components so CLI/AI
    // sees all 4 widget sources in a single discovery call.
    let ext_widgets = match client.get("/extensions/dashboard-components").await {
        Ok(ext_data) => summarize_extension_widgets(&ext_data),
        Err(_) => Vec::new(),
    };
    let ext_count = ext_widgets.len();

    let widgets = extract_list_array(&data, "components");

    let Some(widgets) = widgets else {
        return Ok(CliResponse::success(data, "Widgets listed"));
    };

    let total = widgets.len();
    let summary: Vec<serde_json::Value> = widgets
        .iter()
        .map(|w| {
            let mut entry = json!({
                "id": w.get("id").and_then(|v| v.as_str()).unwrap_or("?"),
                "name": w.get("name").map(display_i18n_name).unwrap_or_else(|| "(unnamed)".to_string()),
                "category": w.get("category").and_then(|v| v.as_str()).unwrap_or("unknown"),
                "version": w.get("version").and_then(|v| v.as_str()).unwrap_or("?"),
            });
            if let Some(src) = w.get("source").and_then(|v| v.as_str()) {
                entry["source"] = json!(src);
            }
            entry
        })
        .collect();

    // Built-in types come straight from the API response (sourced from
    // `neomind_core::dashboard::BUILTIN_WIDGET_TYPES` on the server) — the
    // CLI never hardcodes widget type names itself.
    let builtin_raw = data
        .get("builtin_types")
        .or_else(|| data.get("data").and_then(|d| d.get("builtin_types")))
        .cloned()
        .unwrap_or_else(|| json!([]));
    let builtin_count = data
        .get("builtin_count")
        .and_then(|v| v.as_u64())
        .or_else(|| {
            data.get("data")
                .and_then(|d| d.get("builtin_count"))
                .and_then(|v| v.as_u64())
        })
        .unwrap_or(0);

    Ok(CliResponse::success(
        json!({
            "total": total,
            "widgets": summary,
            "extension_widgets": ext_widgets,
            "extension_count": ext_count,
            "builtin_types": builtin_raw,
            "builtin_count": builtin_count,
        }),
        format!(
            "{} custom/market widget(s), {} extension widget(s), and {} built-in types (use `neomind widget get <id>` for config_schema & usage_example, or `neomind widget market-list` to browse installable marketplace widgets)",
            total, ext_count, builtin_count,
        ),
    ))
}

/// Helper: extract an array from API response, trying common nesting patterns.
fn extract_list_array(data: &serde_json::Value, key: &str) -> Option<Vec<serde_json::Value>> {
    data.as_array()
        .cloned()
        .or_else(|| data.get(key).and_then(|v| v.as_array()).cloned())
        .or_else(|| data.get("data").and_then(|d| d.as_array()).cloned())
        .or_else(|| {
            data.get("data")
                .and_then(|d| d.get(key))
                .and_then(|v| v.as_array())
                .cloned()
        })
}

/// Get widget details (manifest, config_schema, data_binding, size_constraints) by ID.
/// Checks built-in + installed custom/marketplace widgets first, then falls back
/// to extension-provided dashboard components (`/extensions/dashboard-components`).
pub async fn get_widget(client: &ApiClient, id: &str) -> Result<CliResponse> {
    let mut data = match client.get(&format!("/frontend-components/{}", id)).await {
        Ok(d) => d,
        Err(primary_err) => {
            if let Ok(ext_data) = client.get("/extensions/dashboard-components").await {
                if let Some(ext_widget) = find_extension_widget(&ext_data, id) {
                    let usage_example = build_usage_example(id, &ext_widget, "extension");
                    return Ok(CliResponse::success(
                        json!({
                            "component": ext_widget,
                            "source": "extension",
                            "registered": true,
                            "usage_example": usage_example,
                        }),
                        "Widget retrieved (extension-provided component, ready to use)",
                    ));
                }
            }
            return Err(primary_err);
        }
    };

    // Attach a copy-ready `usage_example` payload for `neomind dashboard add-components`
    if let Some(component) = data.get("component").cloned() {
        let source = if data.get("builtin").and_then(|v| v.as_bool()) == Some(true) {
            "builtin"
        } else {
            component
                .get("source")
                .and_then(|v| v.as_str())
                .unwrap_or("custom")
        };
        let usage_example = build_usage_example(id, &component, source);
        if let Some(obj) = data.as_object_mut() {
            obj.insert("usage_example".to_string(), usage_example);
        }
    }

    // Scaffolds land in the same directory the store serves, so a bare
    // `widget create` output answers `get` BEFORE registration — surface
    // the difference instead of letting "get succeeded" read as
    // "installed" (live: round 7 concluded created == installed and
    // skipped the install step). Only act on the server's EXPLICIT
    // provenance flag: a server that predates it stays silent rather than
    // mislabeling registered widgets.
    let msg = match data.get("registered").and_then(|v| v.as_bool()) {
        Some(true) if data.get("builtin").and_then(|v| v.as_bool()) == Some(true) => {
            "Widget retrieved (built-in component, always available)"
        }
        Some(true) => "Widget retrieved (registered)",
        Some(false) => {
            "Widget retrieved — SCAFFOLD ONLY, not registered: run \
             `neomind widget install <id>` to register it before adding it to a dashboard"
        }
        None => "Widget retrieved",
    };
    Ok(CliResponse::success(data, msg))
}

/// Get widget bundle by ID
pub async fn get_widget_bundle(client: &ApiClient, id: &str) -> Result<CliResponse> {
    let data = client
        .get(&format!("/frontend-components/{}/bundle", id))
        .await?;
    Ok(CliResponse::success(data, "Widget bundle retrieved"))
}

/// Install widget from a directory (containing manifest.json + bundle.js) or a ZIP file.
///
/// - If `path` is a directory: reads manifest.json + bundle.js and uploads as separate fields.
/// - If `path` is a .zip file: uploads as a `package` field for server-side extraction.
pub async fn install_widget_file(client: &ApiClient, path: &str) -> Result<CliResponse> {
    // Data-relative paths resolve against the server DATA DIR — the model's
    // space-free form. Absolute paths under the macOS data root
    // ("~/Library/Application Support/...") must be shell-QUOTED (the
    // in-process tokenizer splits at unquoted whitespace); relative paths
    // sidestep the trap entirely. A bare WIDGET ID (what `widget list` /
    // `widget get` report) additionally resolves into the store directory —
    // callers should not need to know the layout (live: round 7 ran
    // `widget install gauge-3832` right after `widget get gauge-3832`
    // succeeded and hit PATH_NOT_FOUND).
    //
    // The dir comes from the FULL resolver (env → desktop app dir → ./data),
    // not `neomind_core::paths::data_dir()` (env or literal "data"): that
    // one is right inside the server process (env set) but a standalone
    // `neomind` terminal invocation falls back to cwd-relative "data" and
    // misses the desktop store entirely — `widget create` (which uses the
    // full resolver) then writes where `widget install` cannot find (live
    // smoke test caught exactly this split).
    let resolved;
    let p: &Path = if Path::new(path).is_absolute() || Path::new(path).exists() {
        Path::new(path)
    } else {
        let data_dir = std::path::PathBuf::from(crate::auto_auth::data_dir_for_paths());
        let by_data = data_dir.join(path);
        let by_id = data_dir.join("frontend-components").join(path);
        resolved = if by_data.exists() {
            by_data
        } else if by_id.exists() {
            by_id
        } else {
            return Ok(CliResponse::error(
                format!(
                    "Path not found: {}. Tried: as given, data-relative ({}), widget id ({}). \
                     Usage: neomind widget install <directory | widget-id | file.zip>",
                    path,
                    by_data.display(),
                    by_id.display()
                ),
                "PATH_NOT_FOUND",
            ));
        };
        resolved.as_path()
    };

    // Absolute paths skip the resolution branches above, so a mistyped
    // absolute path still needs its own existence check — without this it
    // fell through to the zip-extension check and reported a misleading
    // INVALID_PATH_TYPE instead of PATH_NOT_FOUND.
    if !p.exists() {
        return Ok(CliResponse::error(
            format!("Path not found: {}", p.display()),
            "PATH_NOT_FOUND",
        ));
    }

    let data = if p.is_dir() {
        // Directory mode: read manifest.json + bundle.js, upload as separate multipart fields
        let manifest_path = p.join("manifest.json");
        let bundle_path = p.join("bundle.js");

        if !manifest_path.exists() {
            return Ok(CliResponse::error(
                format!("No manifest.json found in {}", path),
                "MANIFEST_MISSING",
            ));
        }
        if !bundle_path.exists() {
            return Ok(CliResponse::error(
                format!("No bundle.js found in {}", path),
                "BUNDLE_MISSING",
            ));
        }

        let manifest_bytes = fs::read(&manifest_path)?;
        let bundle_bytes = fs::read(&bundle_path)?;

        // API expects text manifest + binary bundle
        let manifest_text = String::from_utf8(manifest_bytes)
            .map_err(|e| anyhow::anyhow!("manifest.json is not valid UTF-8: {}", e))?;

        client
            .post_multipart(
                "/frontend-components",
                vec![
                    (
                        "manifest",
                        manifest_text.into_bytes(),
                        "manifest.json".to_string(),
                    ),
                    ("bundle", bundle_bytes, "bundle.js".to_string()),
                ],
            )
            .await?
    } else {
        // File mode: upload as ZIP package
        let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
        if ext != "zip" {
            return Ok(CliResponse::error(
                format!(
                    "Expected a directory or .zip file, got: {}. \
                     Usage: neomind widget install <directory> OR neomind widget install <file.zip>",
                    path
                ),
                "INVALID_PATH_TYPE",
            ));
        }
        let zip_bytes = fs::read(p)?;
        client
            .post_multipart(
                "/frontend-components",
                vec![(
                    "package",
                    zip_bytes,
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("package.zip")
                        .to_string(),
                )],
            )
            .await?
    };

    // API returns {"component": {...}} — extract from wrapper
    let component = data.get("component").cloned().unwrap_or(data.clone());

    Ok(CliResponse::success(component, "Widget installed"))
}

/// Uninstall widget
pub async fn uninstall_widget(client: &ApiClient, id: &str) -> Result<CliResponse> {
    client
        .delete(&format!("/frontend-components/{}", id))
        .await?;
    Ok(CliResponse::success(
        json!({ "id": id }),
        "Widget uninstalled",
    ))
}

/// List marketplace widgets
pub async fn list_marketplace_widgets(client: &ApiClient) -> Result<CliResponse> {
    let data = client.get("/frontend-components/market/list").await?;
    Ok(CliResponse::success(data, "Marketplace widgets listed"))
}

/// Install widget from marketplace
pub async fn install_widget_market(
    client: &ApiClient,
    widget_id: &str,
    version: Option<&str>,
) -> Result<CliResponse> {
    let mut body = json!({
        "component_id": widget_id,
    });
    if let Some(v) = version {
        body["version"] = json!(v);
    }

    let data = client
        .post("/frontend-components/market/install", &body)
        .await?;

    Ok(CliResponse::success(
        data,
        "Widget installed from marketplace",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarizes_and_finds_extension_widgets() {
        let payload = json!({
            "data": [
                {
                    "type": "weather-forecast-card",
                    "name": "Weather Card",
                    "description": "5-day weather forecast widget",
                    "category": "extension",
                    "extension_id": "weather-forecast",
                    "has_data_source": true,
                    "config_schema": { "type": "object", "properties": { "city": { "type": "string" } } }
                },
                {
                    "type": "yolo-stream-overlay",
                    "name": { "en": "YOLO Overlay", "zh": "YOLO 叠加" },
                    "category": "media",
                    "extension_id": "yolo-video",
                    "has_data_source": true
                }
            ]
        });

        let summary = summarize_extension_widgets(&payload);
        assert_eq!(summary.len(), 2);
        assert_eq!(summary[0]["id"], "weather-forecast-card");
        assert_eq!(summary[0]["extension_id"], "weather-forecast");
        assert_eq!(summary[0]["source"], "extension");
        assert_eq!(summary[1]["id"], "yolo-stream-overlay");
        assert_eq!(summary[1]["name"], "YOLO Overlay");

        let found = find_extension_widget(&payload, "weather-forecast-card").unwrap();
        assert_eq!(found["config_schema"]["properties"]["city"]["type"], "string");
        assert!(find_extension_widget(&payload, "missing-widget").is_none());
    }

    #[test]
    fn builds_usage_examples_for_complex_and_all_source_widgets() {
        // 1. agent-monitor-widget binds via data_source.type = "agent"
        let agent_mon = json!({
            "id": "agent-monitor-widget",
            "name": "Agent Monitor",
            "has_data_source": true,
            "size_constraints": { "default_w": 6, "default_h": 5 }
        });
        let ex = build_usage_example("agent-monitor-widget", &agent_mon, "builtin");
        let item = &ex["params"][0];
        assert_eq!(item["type"], "agent-monitor-widget");
        assert_eq!(item["position"]["w"], 6);
        assert_eq!(item["data_source"]["type"], "agent");
        assert_eq!(item["data_source"]["agentId"], "<agent-id>");

        // 2. led-indicator gets concrete threshold rules
        let led = json!({
            "id": "led-indicator",
            "name": "LED Indicator",
            "has_data_source": true,
            "default_config": { "config": { "defaultState": "unknown", "size": "md" } }
        });
        let ex_led = build_usage_example("led-indicator", &led, "builtin");
        assert!(ex_led["params"][0]["config"]["rules"].is_array());
        assert_eq!(ex_led["params"][0]["data_source"]["type"], "telemetry");

        // 3. custom-layer & map-display get bindings inside config
        let layer = json!({
            "id": "custom-layer",
            "name": "Custom Layer",
            "has_data_source": false,
            "default_config": { "config": { "backgroundType": "color", "gridSize": 20 } }
        });
        let ex_layer = build_usage_example("custom-layer", &layer, "builtin");
        assert!(ex_layer["params"][0]["config"]["bindings"].is_array());
        assert!(ex_layer["params"][0].get("data_source").is_none());

        // 4. extension widget gets extension-metric data_source + schema props in config
        let ext = json!({
            "type": "weather-forecast-card",
            "name": "Weather Card",
            "extension_id": "weather-forecast",
            "has_data_source": true,
            "default_size": { "w": 6, "h": 4 },
            "config_schema": {
                "type": "object",
                "properties": {
                    "city": { "type": "string", "default": "Beijing" },
                    "units": { "type": "string", "enum": ["metric", "imperial"] }
                }
            }
        });
        let ex_ext = build_usage_example("weather-forecast-card", &ext, "extension");
        let ext_item = &ex_ext["params"][0];
        assert_eq!(ext_item["position"]["w"], 6);
        assert_eq!(ext_item["data_source"]["type"], "extension-metric");
        assert_eq!(ext_item["data_source"]["id"], "weather-forecast");
        assert_eq!(ext_item["config"]["city"], "Beijing");
        assert_eq!(ext_item["config"]["units"], "metric");

        // 5. device-bound community/marketplace widget gets config.deviceBinding
        let market = json!({
            "id": "smart-thermostat-card",
            "name": { "en": "Smart Thermostat", "zh": "智能温控" },
            "has_data_source": false,
            "has_device_binding": true,
            "default_config": { "targetTemp": 22 }
        });
        let ex_market = build_usage_example("smart-thermostat-card", &market, "market");
        assert_eq!(ex_market["params"][0]["title"], "Smart Thermostat");
        assert_eq!(ex_market["params"][0]["config"]["targetTemp"], 22);
        assert_eq!(
            ex_market["params"][0]["config"]["deviceBinding"]["deviceId"],
            "<device-id>"
        );
    }
}

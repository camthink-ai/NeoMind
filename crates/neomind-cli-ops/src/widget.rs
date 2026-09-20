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
            "files": [
                { "name": "manifest.json", "path": manifest_path.display().to_string() },
                { "name": "bundle.js", "path": bundle_path.display().to_string() }
            ]
        }),
        format!("Widget scaffold created in {}", dir_str),
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

/// List all installed widgets with compact summary.
///
/// Returns id, name, type, and version per installed widget, plus the
/// catalogue of built-in widget types (sourced from the backend's
/// `neomind_core::dashboard::BUILTIN_WIDGET_TYPES`, mirrored from
/// `Renderers.tsx::builtInTypes`). Full manifest for an installed widget
/// is available via `neomind widget get <id>`.
pub async fn list_widgets(client: &ApiClient) -> Result<CliResponse> {
    let data = client.get("/frontend-components").await?;

    let widgets = extract_list_array(&data, "components");

    let Some(widgets) = widgets else {
        return Ok(CliResponse::success(data, "Widgets listed"));
    };

    let total = widgets.len();
    let summary: Vec<serde_json::Value> = widgets
        .iter()
        .map(|w| {
            json!({
                "id": w.get("id").and_then(|v| v.as_str()).unwrap_or("?"),
                "name": w.get("name").and_then(|v| v.as_str()).unwrap_or("(unnamed)"),
                "widget_type": w.get("widget_type").or_else(|| w.get("type")).and_then(|v| v.as_str()).unwrap_or("unknown"),
                "version": w.get("version").and_then(|v| v.as_str()).unwrap_or("?"),
            })
        })
        .collect();

    // Built-in types come straight from the API response (sourced from
    // `neomind_core::dashboard::BUILTIN_WIDGET_TYPES` on the server) — the
    // CLI never hardcodes widget type names itself.
    let builtin_raw = data
        .get("data")
        .and_then(|d| d.get("builtin_types"))
        .or_else(|| data.get("builtin_types"))
        .cloned()
        .unwrap_or_else(|| json!([]));
    let builtin_count = data
        .get("data")
        .and_then(|d| d.get("builtin_count"))
        .and_then(|v| v.as_u64())
        .or_else(|| data.get("builtin_count").and_then(|v| v.as_u64()))
        .unwrap_or(0);

    Ok(CliResponse::success(
        json!({
            "total": total,
            "widgets": summary,
            "builtin_types": builtin_raw,
            "builtin_count": builtin_count,
        }),
        format!(
            "{} widget(s) listed (plus {} built-in types)",
            total, builtin_count,
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

/// Get widget by ID
pub async fn get_widget(client: &ApiClient, id: &str) -> Result<CliResponse> {
    let data = client.get(&format!("/frontend-components/{}", id)).await?;
    Ok(CliResponse::success(data, "Widget retrieved"))
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
    let p = Path::new(path);

    if !p.exists() {
        return Ok(CliResponse::error(
            format!("Path not found: {}", path),
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
        "id": widget_id,
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

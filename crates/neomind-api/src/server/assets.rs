//! Embedded static assets for the web UI.
//!
//! This module uses rust-embed to bundle the frontend build output
//! directly into the binary, eliminating the need for separate static files.
//!
//! When the "static" feature is not enabled, serves a simple message
//! indicating that static assets are not available.

use axum::{
    extract::Path,
    http::{header, StatusCode},
    response::IntoResponse,
};

#[cfg(feature = "static")]
use rust_embed::RustEmbed;

#[cfg(feature = "static")]
/// Embedded static files from the web frontend build.
/// The `static/` directory should contain the output of `npm run build` from web/.
#[derive(RustEmbed)]
#[folder = "static/"]
#[prefix = ""]
struct Assets;

/// Fallback to index.html for SPA routing.
/// This ensures that routes like /dashboard work client-side.
#[cfg(feature = "static")]
fn get_asset_path(path: &str) -> String {
    // Remove leading slash
    let path = path.trim_start_matches('/');

    // For empty path or API routes, return index
    if path.is_empty() || path.starts_with("api/") || path.starts_with("ws") {
        return "index.html".to_string();
    }

    // Check if the file exists in assets
    if Assets::get(path).is_some() {
        return path.to_string();
    }

    // For SPA routes, fall back to index.html
    // This handles client-side routing
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    // If no extension, it's likely a SPA route - serve index.html
    if ext.is_empty() {
        return "index.html".to_string();
    }

    // Otherwise, try the path as-is (404 if not found)
    path.to_string()
}

/// Serve an embedded static file.
///
/// # Arguments
/// * `path` - The file path relative to the static directory
///
/// # Behavior
/// - If static assets are embedded, serves them with correct MIME type
/// - If static assets are not embedded, returns a helpful message
/// - For SPA routes, falls back to index.html
#[cfg(feature = "static")]
pub async fn serve_asset(Path(path): Path<String>) -> impl IntoResponse {
    let asset_path = get_asset_path(&path);

    Assets::get(&asset_path)
        .map(|file| {
            let mime = mime_guess::from_path(&asset_path)
                .first_or_octet_stream()
                .to_string();

            // Enable caching for static assets
            let cache_control = if asset_path.ends_with(".html") {
                "no-cache, no-store, must-revalidate"
            } else {
                "public, max-age=31536000, immutable"
            };

            (
                [
                    (header::CONTENT_TYPE, mime),
                    ("cache-control", cache_control),
                ],
                file.data.to_vec(),
            )
                .into_response()
        })
        .unwrap_or_else(|| {
            // File not found - for SPA routes, serve index.html
            if let Some(index) = Assets::get("index.html") {
                let html = String::from_utf8(index.data.to_vec()).unwrap_or_else(|_| {
                    "<!DOCTYPE html><html><body>App loading...</body></html>".to_string()
                });

                // Inject 404 status for actual missing assets
                if asset_path.contains('.') {
                    (StatusCode::NOT_FOUND, html).into_response()
                } else {
                    (StatusCode::OK, [(header::CONTENT_TYPE, "text/html")], html).into_response()
                }
            } else {
                StatusCode::NOT_FOUND.into_response()
            }
        })
}

/// Serve an embedded static file (fallback when static feature is disabled).
#[cfg(not(feature = "static"))]
pub async fn serve_asset(Path(path): Path<String>) -> impl IntoResponse {
    let path = path.trim_start_matches('/');

    // For API routes, return 404 (they should be handled elsewhere)
    if path.starts_with("api/") || path.starts_with("ws") {
        return StatusCode::NOT_FOUND.into_response();
    }

    // Return a helpful message when static assets are not embedded
    let html = r#"
<!DOCTYPE html>
<html>
<head>
    <title>NeoMind - Static Assets Not Available</title>
    <style>
        body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
               max-width: 600px; margin: 100px auto; padding: 20px; line-height: 1.6; }
        h1 { color: #333; }
        .info { background: #f5f5f5; padding: 15px; border-radius: 5px; margin: 20px 0; }
        code { background: #e0e0e0; padding: 2px 6px; border-radius: 3px; }
    </style>
</head>
<body>
    <h1>NeoMind API Server</h1>
    <div class="info">
        <p><strong>Static assets are not embedded.</strong></p>
        <p>To enable the web UI, rebuild with:</p>
        <pre><code>cargo build --features static</code></pre>
        <p>Or build the frontend separately:</p>
        <pre><code>cd web && npm run build && cp -r dist/* ../crates/api/static/</code></pre>
    </div>
    <p>The API server is running. You can access the API endpoints directly.</p>
</body>
</html>
    "#;

    ([(header::CONTENT_TYPE, "text/html")], html.to_string()).into_response()
}

/// Serve the main index.html entry point.
#[cfg(feature = "static")]
pub async fn serve_index() -> impl IntoResponse {
    Assets::get("index.html")
        .map(|file| {
            let html = String::from_utf8(file.data.to_vec())
                .unwrap_or_else(|_| "<!DOCTYPE html><html><body>Error loading app</body></html>".to_string());
            ([(header::CONTENT_TYPE, "text/html")], html)
        })
        .unwrap_or_else(|| ([(header::CONTENT_TYPE, "text/html")], "<!DOCTYPE html><html><body>NeoMind UI not found. Please run: cd web && npm run build</body></html>"))
        .into_response()
}

/// Serve the main index.html entry point (fallback when static feature is disabled).
#[cfg(not(feature = "static"))]
pub async fn serve_index() -> impl IntoResponse {
    let html = r#"
<!DOCTYPE html>
<html>
<head>
    <title>NeoMind</title>
    <style>
        body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
               max-width: 600px; margin: 100px auto; padding: 20px; line-height: 1.6; }
        h1 { color: #333; }
        .info { background: #f5f5f5; padding: 15px; border-radius: 5px; margin: 20px 0; }
        code { background: #e0e0e0; padding: 2px 6px; border-radius: 3px; }
    </style>
</head>
<body>
    <h1>NeoMind API Server</h1>
    <div class="info">
        <p><strong>Static assets are not embedded.</strong></p>
        <p>To enable the web UI, rebuild with:</p>
        <pre><code>cargo build --features static</code></pre>
    </div>
    <p>The API server is running. Access the API via <code>/api/*</code> endpoints.</p>
</body>
</html>
    "#;

    ([(header::CONTENT_TYPE, "text/html")], html.to_string()).into_response()
}

/// Check if embedded assets are available.
#[cfg(feature = "static")]
pub fn has_embedded_assets() -> bool {
    Assets::get("index.html").is_some()
}

/// Check if embedded assets are available (fallback when static feature is disabled).
#[cfg(not(feature = "static"))]
pub fn has_embedded_assets() -> bool {
    false
}

/// Get a list of embedded asset paths (for debugging).
#[cfg(feature = "static")]
pub fn list_embedded_assets() -> Vec<&'static str> {
    // rust-embed doesn't provide a way to list all files at compile time
    // This is a placeholder for potential future use
    vec!["index.html"]
}

/// Get a list of embedded asset paths (fallback when static feature is disabled).
#[cfg(not(feature = "static"))]
pub fn list_embedded_assets() -> Vec<&'static str> {
    vec![]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_has_embedded_assets() {
        // Should return a boolean without panicking
        let _ = has_embedded_assets();
    }

    #[test]
    fn test_list_embedded_assets() {
        // Should return a vector without panicking
        let _ = list_embedded_assets();
    }

    #[cfg(feature = "static")]
    #[test]
    fn test_get_asset_path() {
        // Root path should serve index.html
        assert_eq!(get_asset_path("/"), "index.html");
        assert_eq!(get_asset_path(""), "index.html");

        // API routes should serve index.html (for SPA)
        assert_eq!(get_asset_path("/api/test"), "index.html");

        // Known assets should use their path
        assert_eq!(get_asset_path("/assets/index.js"), "assets/index.js");

        // SPA routes without extension should serve index.html
        assert_eq!(get_asset_path("/dashboard"), "index.html");
        assert_eq!(get_asset_path("/devices"), "index.html");
    }
}

//! CI drift guard: the /api/docs route table must match the router's actual
//! registrations. The table in `handlers/api_docs.rs` is generated from
//! router.rs; this test re-derives the truth the same way and fails when
//! someone adds a route without refreshing the docs (the CLI promises
//! /api/docs — letting it silently rot is how we got a 404 swagger before).
//!
//! Regenerate the table with the extractor snippet in the PR that added
//! this test (regex over router.rs, dedupe, paste into ROUTES).
//!
//! The extraction must be chain-aware: axum allows several methods on one
//! registration — `.route("/x/:id", put(h).delete(h))` — and the original
//! single-method regex only saw `put`. That is how 19 routed operations
//! (every `delete`/`patch`/second `put` in a chain) went missing from
//! /api/docs while this guard still passed.

use neomind_api::handlers::api_docs::ROUTES;

/// Depth-aware `.route("path", METHOD(h)…METHOD(h))` extraction: returns the
/// path plus EVERY method chained on that registration, not just the first.
/// Parentheses inside string literals and `//` comments are skipped so
/// chained handlers and annotating comments don't confuse the scan.
fn route_calls(section: &str) -> Vec<(String, Vec<String>)> {
    let bytes = section.as_bytes();
    let method_re = regex::Regex::new(r"\b(get|post|put|delete|patch|any)\s*\(").unwrap();
    let mut out = Vec::new();
    let mut i = 0usize;
    while let Some(rel) = section[i..].find(".route(") {
        let open = i + rel + ".route(".len();
        i = open;
        // path string follows the opening paren (whitespace-tolerant)
        let mut j = open;
        while j < bytes.len() && matches!(bytes[j], b' ' | b'\n' | b'\t' | b'\r') {
            j += 1;
        }
        if j >= bytes.len() || bytes[j] != b'"' {
            continue;
        }
        let Some(path_end) = section[j + 1..].find('"') else {
            break;
        };
        let path = section[j + 1..j + 1 + path_end].to_string();
        // scan to the matching ')' of this .route() call
        let mut depth = 1i32;
        let mut k = j + 1 + path_end + 1;
        while k < bytes.len() && depth > 0 {
            match bytes[k] {
                b'"' => match section[k + 1..].find('"') {
                    Some(e) => k += e + 1,
                    None => break,
                },
                b'/' if k > 0 && bytes[k - 1] == b'/' => {
                    if let Some(e) = section[k..].find('\n') {
                        k += e; // sit on the newline; k += 1 below moves past
                    } else {
                        break;
                    }
                }
                b'(' => depth += 1,
                b')' => depth -= 1,
                _ => {}
            }
            k += 1;
        }
        let span = &section[j + 1 + path_end + 1..k.saturating_sub(1)];
        let methods: Vec<String> = {
            let mut ms: Vec<String> = span
                .lines()
                .map(|l| l.split("//").next().unwrap_or(l))
                .flat_map(|l| method_re.captures_iter(l).map(|c| c[1].to_uppercase()))
                .collect();
            ms.sort();
            ms.dedup();
            ms
        };
        if !methods.is_empty() {
            out.push((path, methods));
        }
    }
    out
}

#[test]
fn docs_table_matches_router_registrations() {
    let src = std::fs::read_to_string("src/server/router.rs")
        .or_else(|_| std::fs::read_to_string("crates/neomind-api/src/server/router.rs"))
        .expect("router.rs readable");

    // Same extraction as the generator, but chain-aware: every method
    // chained on one .route() registration counts.
    //
    // EVERY router must be listed, and the coverage assertion below enforces
    // it. The first version of this test used only the five names the
    // generator used — sharing its blind spot — so 14 routes registered in
    // admin/upload routers were absent from /api/docs while this test passed.
    let auth_map = [
        ("public_routes", "public"),
        ("jwt_routes", "jwt-or-api-key"),
        ("websocket_routes", "ws"),
        ("webhook_routes", "webhook"),
        ("protected_routes", "jwt-or-api-key"),
        ("admin_routes", "jwt-only"),
        ("extension_upload_routes", "jwt-or-api-key"),
        ("component_upload_routes", "jwt-or-api-key"),
        ("debug_routes", "debug"),
        ("limited_routes", "jwt-or-api-key"),
    ];

    // Any `let X = Router::new()` not in the map fails loudly.
    let declared: Vec<String> = {
        let re = regex::Regex::new(r"let (\w+)\s*=\s*Router::new\(\)").unwrap();
        re.captures_iter(&src).map(|c| c[1].to_string()).collect()
    };
    let mapped: std::collections::HashSet<&str> = auth_map.iter().map(|(n, _)| *n).collect();
    let unmapped: Vec<&String> = declared
        .iter()
        .filter(|n| !mapped.contains(n.as_str()))
        .collect();
    assert!(
        unmapped.is_empty(),
        "router(s) not covered by the docs table: {unmapped:?} — add them to auth_map \
         here AND to the generator table in handlers/api_docs.rs"
    );

    let mut expected: std::collections::HashSet<(String, String)> = Default::default();
    let mut expected_auth: std::collections::HashMap<(String, String), String> = Default::default();
    for (var, cls) in &auth_map {
        let marker = format!("let {var} = Router::new()");
        let Some(start) = src.find(&marker) else {
            continue;
        };
        let start = start + marker.len();
        let end = src[start..]
            .find("\n    let ")
            .map(|e| start + e)
            .unwrap_or(src.len());
        for (path, methods) in route_calls(&src[start..end]) {
            for method in methods {
                let key = (method, path.clone());
                expected.insert(key.clone());
                // later router wins, mirroring axum merge order
                expected_auth.insert(key, cls.to_string());
            }
        }
    }

    let documented: std::collections::HashSet<(String, String)> = ROUTES
        .iter()
        .map(|r| (r.method.to_string(), r.path.to_string()))
        .collect();

    // Auth class must match too — method+path alone lets a wrong class ship.
    let auth_mismatches: Vec<_> = ROUTES
        .iter()
        .filter_map(|r| {
            let key = (r.method.to_string(), r.path.to_string());
            expected_auth
                .get(&key)
                .filter(|want| want.as_str() != r.auth)
                .map(|want| (key, want.clone(), r.auth))
        })
        .collect();
    assert!(
        auth_mismatches.is_empty(),
        "auth class mismatches (route, expected, documented): {auth_mismatches:?}"
    );

    let missing: Vec<_> = expected.difference(&documented).collect();
    let stale: Vec<_> = documented.difference(&expected).collect();
    assert!(
        missing.is_empty() && stale.is_empty(),
        "\nrouter routes missing from /api/docs table: {missing:?}\n\
         docs entries not in router (stale): {stale:?}\n\
         → regenerate the ROUTES table in handlers/api_docs.rs"
    );
}

#[test]
fn openapi_spec_paths_are_all_routed() {
    let src = std::fs::read_to_string("src/server/router.rs")
        .or_else(|_| std::fs::read_to_string("crates/neomind-api/src/server/router.rs"))
        .expect("router.rs readable");
    let spec = <neomind_api::handlers::openapi::ApiDoc as utoipa::OpenApi>::openapi();
    let mut missing: Vec<String> = Vec::new();
    for path in spec.paths.paths.keys() {
        let utoipa_path = path.clone(); // annotations already carry the /api prefix
                                        // utoipa emits {param}; the router uses :param — normalize for the check.
        let router_style = utoipa_path.replace('{', ":").replace('}', "");
        if !src.contains(&router_style) {
            missing.push(utoipa_path);
        }
    }
    assert!(
        missing.is_empty(),
        "annotated paths not found in router.rs: {missing:?}"
    );
}

/// The Scalar console shell is inline JS: one unbalanced quote and the
/// whole script is a syntax error — the browser shows a BLANK page while
/// every curl-based check stays green (exactly how a blank /api/docs
/// shipped). Guard the rendered HTML structurally.
#[tokio::test]
async fn docs_html_boots_scalar() {
    let axum::response::Html(body) = neomind_api::handlers::api_docs::docs_handler().await;
    assert!(
        body.contains("Scalar.createApiReference"),
        "Scalar bootstrap call missing"
    );
    assert!(
        body.contains("url: '/api/docs/openapi.json'"),
        "spec URL must be a CLOSED string literal — an unterminated one \
         blanks the console"
    );
    assert_eq!(
        body.matches('\'').count() % 2,
        0,
        "unbalanced single quotes in the Scalar shell (syntax error → blank page)"
    );
    assert!(
        body.contains("@scalar/api-reference"),
        "Scalar runtime not loaded from CDN"
    );
}

//! CI drift guard: the /api/docs route table must match the router's actual
//! registrations. The table in `handlers/api_docs.rs` is generated from
//! router.rs; this test re-derives the truth the same way and fails when
//! someone adds a route without refreshing the docs (the CLI promises
//! /api/docs — letting it silently rot is how we got a 404 swagger before).
//!
//! Regenerate the table with the extractor snippet in the PR that added
//! this test (regex over router.rs, dedupe, paste into ROUTES).

use neomind_api::handlers::api_docs::ROUTES;

#[test]
fn docs_table_matches_router_registrations() {
    let src = std::fs::read_to_string("src/server/router.rs")
        .or_else(|_| std::fs::read_to_string("crates/neomind-api/src/server/router.rs"))
        .expect("router.rs readable");

    // Same extraction as the generator: per named Router::new() segment,
    // regex every .route("path", method(handler)) — whitespace-tolerant.
    let auth_map = [
        ("public_routes", "public"),
        ("jwt_routes", "jwt-or-api-key"),
        ("websocket_routes", "ws"),
        ("webhook_routes", "webhook"),
        ("protected_routes", "jwt-or-api-key"),
    ];
    let route_re =
        regex::Regex::new(r#"\.route\(\s*"([^"]+)"\s*,\s*(get|post|put|delete|patch|any)\("#)
            .unwrap();

    let mut expected: std::collections::HashSet<(String, String)> = Default::default();
    for (var, _cls) in &auth_map {
        let marker = format!("let {var} = Router::new()");
        let Some(start) = src.find(&marker) else {
            continue;
        };
        let start = start + marker.len();
        let end = src[start..]
            .find("\n    let ")
            .map(|e| start + e)
            .unwrap_or(src.len());
        for cap in route_re.captures_iter(&src[start..end]) {
            expected.insert((cap[2].to_uppercase(), cap[1].to_string()));
        }
    }

    let documented: std::collections::HashSet<(String, String)> = ROUTES
        .iter()
        .map(|r| (r.method.to_string(), r.path.to_string()))
        .collect();

    // The /api/docs routes themselves: registered but deliberately absent
    // from the table (self-reference noise).
    for extra in [
        ("GET".to_string(), "/api/docs".to_string()),
        ("GET".to_string(), "/api/docs/routes.json".to_string()),
        ("GET".to_string(), "/api/docs/*rest".to_string()),
    ] {
        expected.remove(&extra);
    }

    let missing: Vec<_> = expected.difference(&documented).collect();
    let stale: Vec<_> = documented.difference(&expected).collect();
    assert!(
        missing.is_empty() && stale.is_empty(),
        "\nrouter routes missing from /api/docs table: {missing:?}\n\
         docs entries not in router (stale): {stale:?}\n\
         → regenerate the ROUTES table in handlers/api_docs.rs"
    );
}

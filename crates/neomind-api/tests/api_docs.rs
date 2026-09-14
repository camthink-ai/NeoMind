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
    let route_re =
        regex::Regex::new(r#"\.route\(\s*"([^"]+)"\s*,\s*(get|post|put|delete|patch|any)\("#)
            .unwrap();

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
        for cap in route_re.captures_iter(&src[start..end]) {
            let key = (cap[2].to_uppercase(), cap[1].to_string());
            expected.insert(key.clone());
            // later router wins, mirroring axum merge order
            expected_auth.insert(key, cls.to_string());
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

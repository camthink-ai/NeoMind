//! Guard for the `static` feature.
//!
//! `src/server/assets.rs` embeds `crates/neomind-api/static/` with `rust-embed`
//! at **compile time**, and nothing copies the frontend build there — so a
//! directory left over from an earlier session gets baked into the binary and
//! the server serves a UI that no longer matches the source. That is not
//! hypothetical: the copy in the tree on 2026-09-24 was three days behind
//! `web/dist` and still carried translation keys that had been deleted.
//!
//! This fails the build instead of warning. A stale embed is invisible until
//! somebody reads the wrong page, and the fix is one command away.

use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=static");

    // Only the `static` feature turns the embed on; without it nothing here
    // applies and the build stays free of the check.
    if std::env::var_os("CARGO_FEATURE_STATIC").is_none() {
        return;
    }

    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let embedded = manifest.join("static/index.html");
    let built = manifest.join("../../web/dist/index.html");

    if !embedded.exists() {
        panic!(
            "the `static` feature embeds crates/neomind-api/static/ at compile time, and it \
             has no index.html.\nBuild the frontend into it first:\n    \
             ./scripts/build-smoke.sh\nor by hand:\n    \
             cd web && npm run build && rsync -a --delete dist/ ../crates/neomind-api/static/"
        );
    }

    // Compared by content, not mtime: `rsync -a` preserves timestamps, so after
    // a sync the two files carry the SAME mtime and an mtime test cannot tell a
    // fresh copy from a stale one. index.html names its hashed bundles, so
    // different bytes mean a different build.
    let (Ok(embedded_bytes), Ok(built_bytes)) = (std::fs::read(&embedded), std::fs::read(&built)) else {
        return; // no web/dist next to us; nothing to compare against
    };

    if embedded_bytes != built_bytes {
        panic!(
            "crates/neomind-api/static/ holds a different build from web/dist/, so the binary \
             would serve a UI that does not match the source.\nRe-sync:\n    \
             rsync -a --delete web/dist/ crates/neomind-api/static/"
        );
    }
}

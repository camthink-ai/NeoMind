//! Data-directory resolution for the API layer — thin delegate to the
//! canonical [`neomind_core::paths`] module (kept as a module so existing
//! `crate::server::paths` imports stay valid and the migration history is
//! discoverable from the API crate).

pub use neomind_core::paths::{data_dir, store_path};

/// Path of the extension record store (see the core module docs for the
/// legacy-compat semantics).
pub fn extension_store_path() -> std::path::PathBuf {
    store_path("extensions.redb")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_store_resolves_via_core() {
        // Env-independent sanity: the filename is appended to a data dir.
        let p = extension_store_path();
        assert!(p.ends_with("extensions.redb"));
    }
}

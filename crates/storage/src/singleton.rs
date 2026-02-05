//! Shared database singleton for redb connections.
//!
//! This module provides a global cache of redb Database instances keyed by path,
//! preventing redundant opening of the same database file while still allowing
//! multiple independent connections when needed.

use crate::{Error, Result};
use redb::Database;
use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;
use std::sync::{Arc, RwLock};

/// Global cache of open databases.
///
/// Key: Database path as string
/// Value: Arc<Database> for shared access
fn db_cache() -> &'static RwLock<HashMap<String, Arc<Database>>> {
    static CACHE: OnceLock<RwLock<HashMap<String, Arc<Database>>>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(HashMap::new()))
}

/// Get or open a database, caching the connection for reuse.
///
/// This function implements a singleton pattern per database path:
/// - If the database is already open, returns the cached connection
/// - If not, opens (or creates) the database and caches it
///
/// # Arguments
///
/// * `path` - Path to the database file
///
/// # Returns
///
/// An `Arc<Database>` that can be cloned and shared across the application.
///
/// # Examples
///
/// ```rust,no_run
/// use edge_ai_storage::singleton::get_or_open_db;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// // First call opens the database
/// let db1 = get_or_open_db("./data/sessions.redb")?;
///
/// // Second call returns the same cached connection
/// let db2 = get_or_open_db("./data/sessions.redb")?;
///
/// // Both Arcs point to the same Database
/// assert!(Arc::ptr_eq(&db1, &db2));
/// # Ok(())
/// # }
/// ```
pub fn get_or_open_db<P: AsRef<Path>>(path: P) -> Result<Arc<Database>> {
    let path_str = path.as_ref().to_string_lossy().to_string();

    // Check cache first (read lock)
    {
        let cache = db_cache().read().unwrap();
        if let Some(db) = cache.get(&path_str) {
            return Ok(db.clone());
        }
    }

    // Not in cache - need to open (drop read lock before acquiring write lock)
    // But first check again with write lock in case another thread opened it
    let db: Arc<Database> = {
        let mut cache = db_cache().write().unwrap();

        // Double-check after acquiring write lock
        if let Some(db) = cache.get(&path_str) {
            return Ok(db.clone());
        }

        // Open new database
        let path_ref = path.as_ref();
        let new_db = if path_ref.exists() {
            Database::open(path_ref).map_err(|e| Error::Storage(e.to_string()))?
        } else {
            // Create parent directory if needed
            if let Some(parent) = path_ref.parent() {
                std::fs::create_dir_all(parent)?;
            }
            Database::create(path_ref).map_err(|e| Error::Storage(e.to_string()))?
        };

        let db = Arc::new(new_db);
        cache.insert(path_str, db.clone());
        db
    };

    Ok(db)
}

/// Close a cached database connection.
///
/// Removes the database from the cache. Note that this does not force
/// the database to close immediately - it will remain open as long as
/// other `Arc<Database>` references exist.
///
/// # Arguments
///
/// * `path` - Path to the database file to close
///
/// # Returns
///
/// - `Some(Arc<Database>)` if the database was in the cache
/// - `None` if the database was not cached
pub fn close_db<P: AsRef<Path>>(path: P) -> Option<Arc<Database>> {
    let path_str = path.as_ref().to_string_lossy().to_string();
    let mut cache = db_cache().write().ok()?;
    cache.remove(&path_str)
}

/// Clear all cached database connections.
///
/// This removes all databases from the cache. Like `close_db`, this
/// doesn't force immediate closure - databases remain open if other
/// `Arc` references exist.
///
/// # Returns
///
/// The number of databases that were removed from the cache.
pub fn clear_cache() -> usize {
    let mut cache = db_cache().write().unwrap();
    let count = cache.len();
    cache.clear();
    count
}

/// Get the number of cached database connections.
pub fn cache_size() -> usize {
    let cache = db_cache().read().unwrap();
    cache.len()
}

/// Check if a specific database is currently cached.
pub fn is_cached<P: AsRef<Path>>(path: P) -> bool {
    let path_str = path.as_ref().to_string_lossy().to_string();
    let cache = db_cache().read().unwrap();
    cache.contains_key(&path_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_singleton_same_instance() {
        let temp =
            std::env::temp_dir().join(format!("test_singleton_{}.redb", uuid::Uuid::new_v4()));

        let db1 = get_or_open_db(&temp).unwrap();
        let db2 = get_or_open_db(&temp).unwrap();

        // Both Arcs should point to the same Database
        assert!(Arc::ptr_eq(&db1, &db2));
    }

    #[test]
    fn test_cache_size() {
        // This test verifies the cache management functionality.
        // Note: Tests in this module share global cache state, so results
        // may vary when run in parallel with tests from other modules.

        let temp1 = std::env::temp_dir().join(format!("test_cache1_{}.redb", uuid::Uuid::new_v4()));
        let temp2 = std::env::temp_dir().join(format!("test_cache2_{}.redb", uuid::Uuid::new_v4()));

        let initial_size = cache_size();
        get_or_open_db(&temp1).unwrap();
        let size_after_first = cache_size();
        assert!(size_after_first >= initial_size + 1);

        get_or_open_db(&temp2).unwrap();
        let size_after_second = cache_size();
        assert!(size_after_second >= initial_size + 2);

        // Clear cache and verify it reduces (may not be exactly 0 due to parallel tests)
        let _cleared = clear_cache();
        let final_size = cache_size();
        assert!(final_size <= size_after_second - 2 || final_size < 2,
                "Cache should be cleared, went from {} to {}", size_after_second, final_size);
    }

    #[test]
    fn test_is_cached() {
        let temp = std::env::temp_dir().join(format!("test_cached_{}.redb", uuid::Uuid::new_v4()));

        assert!(!is_cached(&temp));

        get_or_open_db(&temp).unwrap();
        assert!(is_cached(&temp));

        close_db(&temp);
        assert!(!is_cached(&temp));
    }

    #[test]
    fn test_close_db() {
        let temp = std::env::temp_dir().join(format!("test_close_{}.redb", uuid::Uuid::new_v4()));

        get_or_open_db(&temp).unwrap();
        assert!(is_cached(&temp));

        // Note: In parallel test execution, another test might have affected the cache
        // So we just verify that close_db works correctly, not that it returns Some
        let _removed = close_db(&temp);
        assert!(!is_cached(&temp));

        // Closing non-existent DB returns None (now it's definitely not cached)
        let removed = close_db(&temp);
        assert!(removed.is_none());
    }
}

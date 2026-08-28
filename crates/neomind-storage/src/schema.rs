//! Per-database schema version stamp and rollback guard.
//!
//! The storage layer evolves row formats mostly via `#[serde(default)]`
//! (additive changes load old rows fine) — no migration machinery needed for
//! that. What serde does NOT protect against is the reverse: **older code
//! opening newer data**. Unknown fields are silently dropped and new fields
//! default-filled, so a rolled-back install (edge boxes do this) can read a
//! row, save it back, and permanently destroy data — with no error anywhere.
//!
//! Every store stamps its database with the row-format version on first
//! open. Opening a database stamped with a NEWER version refuses instead of
//! corrupting. Older (or unstamped) databases pass through — that's the
//! normal serde-defaults path — and get stamped with the current version.
//!
//! This is deliberately NOT a migration framework. When a breaking row-format
//! change lands someday: bump `CURRENT_SCHEMA_VERSION`, write the one-shot
//! migration in [`check_or_stamp`] keyed on `stored < n`, and test it against
//! a backup (see `backup.rs`). Engine swaps (sled→redb happened once) are
//! out of scope by design — they need per-store export/import code.

use redb::{Database, TableDefinition};

/// Current row-format version for every neomind-storage database.
///
/// Bump ONLY for changes older code cannot safely read — e.g. a field
/// changing meaning where serde defaults would paper over real data loss.
/// Additive fields with `#[serde(default)]` do not need a bump.
pub const CURRENT_SCHEMA_VERSION: u64 = 1;

const SCHEMA_META_TABLE: TableDefinition<&str, u64> = TableDefinition::new("schema_meta");
const VERSION_KEY: &str = "version";

/// Check the database's schema stamp against this build, and stamp it if
/// missing or older.
///
/// Errors when the database was written by a NEWER build (rollback scenario)
/// — callers must treat this as fatal for that database, not fall back to a
/// default-value store handle, or the first save would destroy the new data.
pub fn check_or_stamp(db: &Database) -> Result<(), String> {
    let stored: Option<u64> = {
        let read = db.begin_read().map_err(|e| e.to_string())?;
        match read.open_table(SCHEMA_META_TABLE) {
            Ok(table) => table
                .get(VERSION_KEY)
                .map_err(|e| e.to_string())?
                .map(|v| v.value()),
            Err(_) => None, // table not created yet → unstamped database
        }
    };

    match stored {
        Some(v) if v > CURRENT_SCHEMA_VERSION => Err(format!(
            "database schema version {v} is NEWER than this build supports ({CURRENT_SCHEMA_VERSION}). \
             The data was written by a newer NeoMind — upgrade the server instead of rolling back \
             (opening it now would silently destroy newer-format rows on the next save)."
        )),
        Some(_) | None => {
            // Stamp (idempotent). One-shot migrations keyed on
            // `stored < CURRENT_SCHEMA_VERSION` would run here; none exist yet.
            let write = db.begin_write().map_err(|e| e.to_string())?;
            {
                let mut table = write
                    .open_table(SCHEMA_META_TABLE)
                    .map_err(|e| e.to_string())?;
                table
                    .insert(VERSION_KEY, CURRENT_SCHEMA_VERSION)
                    .map_err(|e| e.to_string())?;
            }
            write.commit().map_err(|e| e.to_string())?;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamps_on_first_open_and_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let db = Database::create(tmp.path().join("a.redb")).unwrap();
        check_or_stamp(&db).unwrap();
        // Second call sees the stamp and passes without changes.
        check_or_stamp(&db).unwrap();
        let read = db.begin_read().unwrap();
        let table = read.open_table(SCHEMA_META_TABLE).unwrap();
        assert_eq!(
            table.get(VERSION_KEY).unwrap().unwrap().value(),
            CURRENT_SCHEMA_VERSION
        );
    }

    #[test]
    fn refuses_newer_version() {
        let tmp = tempfile::tempdir().unwrap();
        let db = Database::create(tmp.path().join("b.redb")).unwrap();
        // Simulate a newer build having stamped this database.
        let write = db.begin_write().unwrap();
        {
            let mut table = write.open_table(SCHEMA_META_TABLE).unwrap();
            table
                .insert(VERSION_KEY, CURRENT_SCHEMA_VERSION + 1)
                .unwrap();
        }
        write.commit().unwrap();

        let err = check_or_stamp(&db).unwrap_err();
        assert!(err.contains("NEWER"), "unexpected error: {err}");
    }
}

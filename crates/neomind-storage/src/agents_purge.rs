//! Offline V1→V2-only purge migration.
//!
//! Distinct from bare schema version `2`: stamps `agent_runtime=v2_only` so the
//! main binary can refuse dual-stack paths after the maintenance window.
//! Idempotent; preserves agent IDs; returns a report of counts/diffs.

use redb::{Database, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};

use crate::agents_v2::{
    ensure_v2_tables, migrate_agents_v1_to_v2, table_exists_agents_v1, AgentStoreV2,
};
use crate::Error;

const RUNTIME_META_TABLE: TableDefinition<&str, &str> = TableDefinition::new("agent_runtime_meta");
const RUNTIME_KEY: &str = "agent_runtime";
/// Distinct migration mark beyond bare schema version 2.
pub const RUNTIME_V2_ONLY: &str = "v2_only";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PurgeReport {
    pub db_path: String,
    pub had_v1_table: bool,
    pub v1_rows_migrated: usize,
    pub v2_specs_after: usize,
    pub deleted_v1_table: bool,
    pub already_v2_only: bool,
    pub runtime_mark: String,
    pub notes: Vec<String>,
}

/// Read the purge mark, if any.
pub fn read_runtime_mark(db: &Database) -> Result<Option<String>, Error> {
    let read = db.begin_read().map_err(|e| Error::Storage(e.to_string()))?;
    match read.open_table(RUNTIME_META_TABLE) {
        Ok(table) => Ok(table
            .get(RUNTIME_KEY)
            .map_err(|e| Error::Storage(e.to_string()))?
            .map(|v| v.value().to_string())),
        Err(_) => Ok(None),
    }
}

pub fn is_v2_only(db: &Database) -> Result<bool, Error> {
    Ok(read_runtime_mark(db)?.as_deref() == Some(RUNTIME_V2_ONLY))
}

fn stamp_v2_only(db: &Database) -> Result<(), Error> {
    let write = db
        .begin_write()
        .map_err(|e| Error::Storage(e.to_string()))?;
    {
        let mut table = write
            .open_table(RUNTIME_META_TABLE)
            .map_err(|e| Error::Storage(e.to_string()))?;
        table
            .insert(RUNTIME_KEY, RUNTIME_V2_ONLY)
            .map_err(|e| Error::Storage(e.to_string()))?;
    }
    write.commit().map_err(|e| Error::Storage(e.to_string()))?;
    Ok(())
}

/// Offline purge: migrate remaining v1 rows → five-table v2, drop `agents`, stamp mark.
///
/// Safe to re-run: already-purged DBs return `already_v2_only=true` with counts.
pub fn purge_agents_to_v2_only(path: &std::path::Path) -> Result<PurgeReport, Error> {
    let mut report = PurgeReport {
        db_path: path.display().to_string(),
        runtime_mark: RUNTIME_V2_ONLY.into(),
        ..Default::default()
    };

    {
        let db = Database::create(path).map_err(|e| Error::Storage(e.to_string()))?;

        if is_v2_only(&db)? && !table_exists_agents_v1(&db).unwrap_or(true) {
            ensure_v2_tables(&db)?;
            report.already_v2_only = true;
            report.notes.push("already stamped v2_only".into());
        } else {
            report.had_v1_table = table_exists_agents_v1(&db).unwrap_or(false);
            let before = if report.had_v1_table {
                count_v1_rows(&db)?
            } else {
                0
            };

            migrate_agents_v1_to_v2(&db).map_err(Error::Storage)?;
            if report.had_v1_table {
                report.v1_rows_migrated = before;
                report.deleted_v1_table = !table_exists_agents_v1(&db).unwrap_or(true);
            } else {
                ensure_v2_tables(&db)?;
                report
                    .notes
                    .push("no v1 table — ensured five v2 tables".into());
            }
            stamp_v2_only(&db)?;
            report.notes.push(format!(
                "preserved ids; migrated={}",
                report.v1_rows_migrated
            ));
        }
        // Drop db before reopening via AgentStoreV2::open.
    }

    let v2 = AgentStoreV2::open(path)?;
    report.v2_specs_after = v2.list_specs()?.len();
    Ok(report)
}

fn count_v1_rows(db: &Database) -> Result<usize, Error> {
    let read = db.begin_read().map_err(|e| Error::Storage(e.to_string()))?;
    let table = read
        .open_table(TableDefinition::<&str, &[u8]>::new("agents"))
        .map_err(|e| Error::Storage(e.to_string()))?;
    let mut n = 0;
    for entry in table.iter().map_err(|e| Error::Storage(e.to_string()))? {
        let _ = entry.map_err(|e| Error::Storage(e.to_string()))?;
        n += 1;
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::{
        AgentMemory, AgentSchedule, AgentStats, AgentStatus, AiAgent, ExecutionMode, ScheduleType,
    };

    fn fixture(id: &str) -> AiAgent {
        AiAgent {
            id: id.into(),
            name: id.into(),
            description: None,
            user_prompt: "brief".into(),
            llm_backend_id: None,
            parsed_intent: None,
            resources: vec![],
            schedule: AgentSchedule {
                schedule_type: ScheduleType::Manual,
                cron_expression: None,
                interval_seconds: None,
                event_filter: None,
                timezone: None,
            },
            status: AgentStatus::Active,
            priority: 100,
            created_at: 1,
            updated_at: 2,
            last_execution_at: None,
            stats: AgentStats::default(),
            memory: AgentMemory::default(),
            conversation_history: vec![],
            user_messages: vec![],
            conversation_summary: None,
            context_window_size: 10,
            enable_tool_chaining: false,
            max_chain_depth: 5,
            tool_config: None,
            execution_mode: ExecutionMode::Structured,
            error_message: None,
            system_prompt: None,
            max_retries: 0,
            consecutive_failures: 0,
            output_schema: None,
            operator_config: None,
            memory_mode: None,
            notify: None,
            max_tokens_per_run: None,
        }
    }

    #[test]
    fn empty_db_purge_stamps_v2_only_without_v1_table() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("agents.redb");
        let report = purge_agents_to_v2_only(&path).unwrap();
        assert!(!report.had_v1_table);
        assert_eq!(report.runtime_mark, RUNTIME_V2_ONLY);
        assert_eq!(report.v2_specs_after, 0);

        // Empty V2-only gate: five tables present, zero specs, no agents table.
        // Drop the Database handle before AgentStoreV2::open (redb exclusive lock).
        {
            let db = Database::open(&path).unwrap();
            assert!(is_v2_only(&db).unwrap());
            assert!(!table_exists_agents_v1(&db).unwrap());
        }
        {
            let v2 = AgentStoreV2::open(&path).unwrap();
            assert!(v2.list_specs().unwrap().is_empty());
        }

        let again = purge_agents_to_v2_only(&path).unwrap();
        assert!(again.already_v2_only);
        assert_eq!(again.v2_specs_after, 0);
    }

    #[test]
    fn purge_preserves_ids_from_v1_rows() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("agents.redb");
        {
            let db = Database::create(&path).unwrap();
            let write = db.begin_write().unwrap();
            {
                let mut t = write
                    .open_table(TableDefinition::<&str, &[u8]>::new("agents"))
                    .unwrap();
                let agent = fixture("keep-me");
                let bytes = serde_json::to_vec(&agent).unwrap();
                t.insert("keep-me", bytes.as_slice()).unwrap();
            }
            write.commit().unwrap();
        }

        let report = purge_agents_to_v2_only(&path).unwrap();
        assert_eq!(report.v1_rows_migrated, 1);
        assert!(report.deleted_v1_table);

        let v2 = AgentStoreV2::open(&path).unwrap();
        let spec = v2.get_spec("keep-me").unwrap().expect("id preserved");
        assert_eq!(spec.id, "keep-me");
        assert_eq!(spec.name, "keep-me");
    }
}

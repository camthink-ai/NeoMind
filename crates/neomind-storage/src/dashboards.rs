//! Dashboard storage using redb.
//!
//! Provides persistent storage for visual dashboards with components.

use parking_lot::Mutex;
use std::path::Path;
use std::sync::Arc;
use std::sync::OnceLock;

use redb::{Database, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use serde_json;

use crate::Error;

// Dashboard table: key = dashboard_id, value = JSON dashboard (serialized)
const DASHBOARDS_TABLE: TableDefinition<&str, Vec<u8>> = TableDefinition::new("dashboards");

// Dashboard index table: key = "default", value = dashboard_id (for default dashboard tracking)
const DEFAULT_TABLE: TableDefinition<&str, &str> = TableDefinition::new("dashboards_default");

// Share tokens table: key = token string, value = JSON ShareToken
const SHARE_TOKENS_TABLE: TableDefinition<&str, Vec<u8>> = TableDefinition::new("share_tokens");

// ============================================================================
// Types
// ============================================================================

/// Dashboard layout configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardLayout {
    pub columns: u32,
    #[serde(alias = "rows")]
    pub rows: RowsValue,
    pub breakpoints: LayoutBreakpoints,
}

/// Rows value - can be "auto" string or a number.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RowsValue {
    String(String),
    Number(u32),
}

/// Layout breakpoints for responsive design.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutBreakpoints {
    pub lg: u32,
    pub md: u32,
    pub sm: u32,
    pub xs: u32,
}

/// Component position on the grid.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentPosition {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_w: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_h: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_w: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_h: Option<u32>,
}

/// Dashboard component.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardComponent {
    pub id: String,
    #[serde(alias = "type", rename = "type")]
    pub component_type: String,
    pub position: ComponentPosition,
    #[serde(skip_serializing_if = "Option::is_none", alias = "title")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "data_source")]
    pub data_source: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "display")]
    pub display: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "config")]
    pub config: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "actions")]
    pub actions: Option<serde_json::Value>,
}

/// Dashboard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dashboard {
    pub id: String,
    pub name: String,
    /// Optional human description. Old rows predate this field — serde
    /// default keeps them readable (None).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub layout: DashboardLayout,
    pub components: Vec<DashboardComponent>,
    #[serde(alias = "created_at")]
    pub created_at: i64,
    #[serde(alias = "updated_at")]
    pub updated_at: i64,
    #[serde(skip_serializing_if = "Option::is_none", alias = "is_default")]
    pub is_default: Option<bool>,
    /// Manual sort order for sidebar display. Lower = higher up. `None` for
    /// legacy rows created before this field existed — they fall to the bottom
    /// of the list via `i32::MAX` tiebreak at the API layer.
    #[serde(skip_serializing_if = "Option::is_none", alias = "sort_order")]
    pub sort_order: Option<i32>,
}

/// Dashboard template.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardTemplate {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    pub layout: DashboardLayout,
    pub components: Vec<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required_resources: Option<RequiredResources>,
}

/// Required resources for a template.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequiredResources {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub devices: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agents: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rules: Option<u32>,
}

impl Dashboard {
    /// Create a new dashboard with generated ID and timestamps.
    pub fn new(name: String, layout: DashboardLayout) -> Self {
        let now = chrono::Utc::now().timestamp();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name,
            description: None,
            layout,
            components: Vec::new(),
            created_at: now,
            updated_at: now,
            is_default: None,
            sort_order: None,
        }
    }

    /// Add a component to the dashboard.
    pub fn with_component(mut self, component: DashboardComponent) -> Self {
        self.components.push(component);
        self.updated_at = chrono::Utc::now().timestamp();
        self
    }

    /// Set as default dashboard.
    pub fn with_default(mut self, is_default: bool) -> Self {
        self.is_default = Some(is_default);
        self
    }
}

impl DashboardLayout {
    /// Create a default 12-column layout.
    pub fn default_layout() -> Self {
        Self {
            columns: 12,
            rows: RowsValue::String("auto".to_string()),
            breakpoints: LayoutBreakpoints {
                lg: 1200,
                md: 996,
                sm: 768,
                xs: 480,
            },
        }
    }
}

/// Dashboard share permissions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharePermissions {
    pub allow_interactive: bool,
}

/// Dashboard share token
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareToken {
    pub token: String,
    pub dashboard_id: String,
    pub permissions: SharePermissions,
    pub created_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
}

/// Static cache for default templates.
/// Initialized once on first access and reused for subsequent calls.
static TEMPLATES_CACHE: OnceLock<Vec<DashboardTemplate>> = OnceLock::new();

/// Default templates.
///
/// Performance optimization: Uses OnceLock for thread-safe lazy initialization.
/// Templates are created only once on first access and cached for lifetime of process.
pub fn default_templates() -> &'static Vec<DashboardTemplate> {
    TEMPLATES_CACHE.get_or_init(|| {
        vec![
            DashboardTemplate {
                id: "overview".to_string(),
                name: "Overview".to_string(),
                description: "System overview with devices, agents, and events".to_string(),
                category: "overview".to_string(),
                icon: Some("LayoutDashboard".to_string()),
                layout: DashboardLayout::default_layout(),
                components: Vec::new(),
                required_resources: Some(RequiredResources {
                    devices: Some(1),
                    agents: Some(1),
                    rules: Some(0),
                }),
            },
            DashboardTemplate {
                id: "blank".to_string(),
                name: "Blank Canvas".to_string(),
                description: "Start from scratch with an empty dashboard".to_string(),
                category: "custom".to_string(),
                icon: Some("Square".to_string()),
                layout: DashboardLayout::default_layout(),
                components: Vec::new(),
                required_resources: Some(RequiredResources {
                    devices: Some(0),
                    agents: Some(0),
                    rules: Some(0),
                }),
            },
        ]
    })
}

// ============================================================================
// Dashboard Store
// ============================================================================

/// Dashboard storage using redb.
pub struct DashboardStore {
    db: Arc<Database>,
    path: String,
}

/// Global dashboard store singleton (thread-safe).
static DASHBOARD_STORE_SINGLETON: Mutex<Option<Arc<DashboardStore>>> = Mutex::new(None);

impl DashboardStore {
    /// Open or create a dashboard store at the given path.
    /// Uses a singleton pattern to prevent multiple opens of the same database.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Arc<Self>, Error> {
        let path_str = path.as_ref().to_string_lossy().to_string();

        // Check if we already have a store for this path
        {
            let singleton = DASHBOARD_STORE_SINGLETON.lock();
            if let Some(store) = singleton.as_ref() {
                if store.path == path_str {
                    return Ok(store.clone());
                }
            }
        }

        // Create new store
        let path_ref = path.as_ref();
        let db = if path_ref.exists() {
            Database::open(path_ref)?
        } else {
            // Ensure parent directory exists
            if let Some(parent) = path_ref.parent() {
                std::fs::create_dir_all(parent)?;
            }
            Database::create(path_ref)?
        };
        // Rollback guard: refuse databases stamped by a newer build (see schema.rs).
        crate::schema::check_or_stamp(&db)
            .map_err(|e| Error::Storage(format!("schema version: {e}")))?;

        let store = Arc::new(DashboardStore {
            db: Arc::new(db),
            path: path_str.clone(),
        });

        {
            let mut singleton = DASHBOARD_STORE_SINGLETON.lock();
            *singleton = Some(store.clone());
        }

        tracing::info!("Dashboard store initialized at {}", path_str);
        Ok(store)
    }

    /// Create an in-memory dashboard store for testing.
    pub fn memory() -> Result<Arc<Self>, Error> {
        let temp_path =
            std::env::temp_dir().join(format!("dashboards_test_{}.redb", uuid::Uuid::new_v4()));
        Self::open(temp_path)
    }

    /// Save a dashboard.
    pub fn save(&self, dashboard: &Dashboard) -> Result<(), Error> {
        let write_txn = self.db.begin_write()?;

        // Serialize dashboard
        let serialized = serde_json::to_vec(dashboard)?;

        {
            let mut table = write_txn.open_table(DASHBOARDS_TABLE)?;
            table.insert(dashboard.id.as_str(), serialized)?;
        }

        write_txn.commit()?;
        Ok(())
    }

    /// Load a dashboard by ID.
    pub fn load(&self, id: &str) -> Result<Option<Dashboard>, Error> {
        let read_txn = self.db.begin_read()?;
        let table = match read_txn.open_table(DASHBOARDS_TABLE) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(None),
            Err(e) => return Err(e.into()),
        };

        match table.get(id)? {
            Some(value) => {
                let dashboard: Dashboard = serde_json::from_slice(value.value().as_slice())?;
                Ok(Some(dashboard))
            }
            None => Ok(None),
        }
    }

    /// List all dashboards.
    ///
    /// Performance optimization: Supports pagination with limit/offset parameters
    /// to avoid loading all dashboards into memory when only a subset is needed.
    pub fn list_all(&self) -> Result<Vec<Dashboard>, Error> {
        self.list_paginated(None, None)
    }

    /// List dashboards with pagination support.
    ///
    /// # Arguments
    /// * `limit` - Maximum number of dashboards to return (None = no limit)
    /// * `offset` - Number of dashboards to skip (None = start from beginning)
    pub fn list_paginated(
        &self,
        limit: Option<usize>,
        offset: Option<usize>,
    ) -> Result<Vec<Dashboard>, Error> {
        let read_txn = self.db.begin_read()?;
        let table = match read_txn.open_table(DASHBOARDS_TABLE) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };

        let mut dashboards = Vec::new();
        let skip_count = offset.unwrap_or(0);
        let max_count = limit.unwrap_or(usize::MAX);

        for (index, result) in table.iter()?.enumerate() {
            // Skip items before offset
            if index < skip_count {
                continue;
            }
            // Stop after reaching limit
            if index >= skip_count + max_count {
                break;
            }

            let (_key, value) = result?;
            let dashboard: Dashboard = serde_json::from_slice(value.value().as_slice())?;
            dashboards.push(dashboard);
        }

        Ok(dashboards)
    }

    /// Delete a dashboard.
    pub fn delete(&self, id: &str) -> Result<(), Error> {
        let write_txn = self.db.begin_write()?;

        {
            let mut table = write_txn.open_table(DASHBOARDS_TABLE)?;
            table.remove(id)?;

            // Drop the default pointer ONLY if it pointed at the deleted
            // dashboard — deleting any non-default board used to clear the
            // global default too, silently losing the user's default page.
            let mut default_table = write_txn.open_table(DEFAULT_TABLE)?;
            let is_default = default_table
                .get("default")?
                .map(|v| v.value() == id)
                .unwrap_or(false);
            if is_default {
                let _ = default_table.remove("default");
            }
        }

        write_txn.commit()?;
        Ok(())
    }

    /// Set a dashboard as the default.
    ///
    /// Performance optimization: Uses a single transaction with batch updates
    /// to avoid N+1 query problem. Only deserializes/serializes data once per dashboard.
    pub fn set_default(&self, id: &str) -> Result<(), Error> {
        let write_txn = self.db.begin_write()?;

        {
            let mut table = write_txn.open_table(DASHBOARDS_TABLE)?;

            // Batch update: unset is_default on all dashboards in a single pass
            // This avoids N+1 queries by processing all updates in one iteration
            let mut dashboards_to_update: Vec<(String, Vec<u8>)> = Vec::new();

            for result in table.iter()? {
                let (key, value) = result?;
                let dashboard_id = key.value().to_string();

                // Parse and update dashboard
                if let Ok(mut dashboard) = serde_json::from_slice::<Dashboard>(&value.value()) {
                    dashboard.is_default = Some(dashboard_id == id);
                    if let Ok(serialized) = serde_json::to_vec(&dashboard) {
                        dashboards_to_update.push((dashboard_id, serialized));
                    }
                }
            }

            // Apply all updates
            for (dashboard_id, serialized) in dashboards_to_update {
                table.insert(dashboard_id.as_str(), serialized)?;
            }

            // Update default index for fast lookup
            let mut default_table = write_txn.open_table(DEFAULT_TABLE)?;
            default_table.insert("default", id)?;
        }

        write_txn.commit()?;
        Ok(())
    }

    /// Return the highest `sort_order` across all dashboards, or 0 if none
    /// have been set / the table is empty. Used to compute the next order
    /// value for newly-created dashboards (appended at the bottom).
    pub fn max_sort_order(&self) -> Result<i32, Error> {
        let read_txn = self.db.begin_read()?;
        let table = match read_txn.open_table(DASHBOARDS_TABLE) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(0),
            Err(e) => return Err(e.into()),
        };
        let mut max: i32 = 0;
        for result in table.iter()? {
            let (_key, value) = result?;
            if let Ok(dashboard) = serde_json::from_slice::<Dashboard>(value.value().as_slice()) {
                if let Some(order) = dashboard.sort_order {
                    if order > max {
                        max = order;
                    }
                }
            }
        }
        Ok(max)
    }

    /// Batch-assign `sort_order` to the supplied `(dashboard_id, order)` pairs
    /// in a single transaction. Pattern mirrors `set_default()`. Used by the
    /// `PUT /api/dashboards/reorder` endpoint.
    pub fn set_sort_orders(&self, items: &[(String, i32)]) -> Result<(), Error> {
        if items.is_empty() {
            return Ok(());
        }
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(DASHBOARDS_TABLE)?;
            for (id, order) in items {
                let bytes: Vec<u8> = {
                    let row = table
                        .get(id.as_str())?
                        .ok_or_else(|| Error::NotFound(format!("Dashboard '{}' not found", id)))?;
                    row.value().to_vec()
                };
                let mut dashboard: Dashboard = serde_json::from_slice(&bytes)?;
                dashboard.sort_order = Some(*order);
                let serialized = serde_json::to_vec(&dashboard)?;
                table.insert(id.as_str(), serialized)?;
            }
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Get the default dashboard ID.
    pub fn get_default_id(&self) -> Result<Option<String>, Error> {
        let read_txn = self.db.begin_read()?;
        let table = match read_txn.open_table(DEFAULT_TABLE) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(None),
            Err(e) => return Err(e.into()),
        };

        match table.get("default")? {
            Some(value) => Ok(Some(value.value().to_string())),
            None => Ok(None),
        }
    }

    /// Get the default dashboard.
    ///
    /// Performance optimization: Uses the default index table for O(1) lookup.
    /// Only falls back to scanning if the index is missing (backward compatibility).
    pub fn get_default(&self) -> Result<Option<Dashboard>, Error> {
        // Fast path: use the default index table
        if let Some(id) = self.get_default_id()? {
            return self.load(&id);
        }

        // Fallback: scan for is_default flag (legacy data)
        // This is only executed once after upgrading to the new indexed format
        let dashboards = self.list_all()?;
        for dashboard in dashboards {
            if dashboard.is_default == Some(true) {
                // Update the index for future fast lookups
                let _ = self.set_default(&dashboard.id);
                return Ok(Some(dashboard));
            }
        }
        Ok(None)
    }

    /// Check if a dashboard exists.
    pub fn exists(&self, id: &str) -> Result<bool, Error> {
        let read_txn = self.db.begin_read()?;
        let table = match read_txn.open_table(DASHBOARDS_TABLE) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(false),
            Err(e) => return Err(e.into()),
        };
        Ok(table.get(id)?.is_some())
    }

    // ========================================================================
    // Share Token methods
    // ========================================================================

    /// Save a share token.
    pub fn save_share_token(&self, share: &ShareToken) -> Result<(), Error> {
        let write_txn = self.db.begin_write()?;
        let serialized = serde_json::to_vec(share)?;
        {
            let mut table = write_txn.open_table(SHARE_TOKENS_TABLE)?;
            table.insert(share.token.as_str(), serialized)?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Load a share token by token string.
    pub fn load_share_token(&self, token: &str) -> Result<Option<ShareToken>, Error> {
        let read_txn = self.db.begin_read()?;
        let table = match read_txn.open_table(SHARE_TOKENS_TABLE) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        match table.get(token)? {
            Some(value) => {
                let share: ShareToken = serde_json::from_slice(value.value().as_slice())?;
                Ok(Some(share))
            }
            None => Ok(None),
        }
    }

    /// Delete a share token.
    pub fn delete_share_token(&self, token: &str) -> Result<(), Error> {
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(SHARE_TOKENS_TABLE)?;
            table.remove(token)?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// List all share tokens for a given dashboard.
    pub fn list_share_tokens(&self, dashboard_id: &str) -> Result<Vec<ShareToken>, Error> {
        let read_txn = self.db.begin_read()?;
        let table = match read_txn.open_table(SHARE_TOKENS_TABLE) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        let mut tokens = Vec::new();
        for result in table.iter()? {
            let (_key, value) = result?;
            if let Ok(share) = serde_json::from_slice::<ShareToken>(value.value().as_slice()) {
                if share.dashboard_id == dashboard_id {
                    tokens.push(share);
                }
            }
        }
        Ok(tokens)
    }

    /// Get dashboard count.
    pub fn count(&self) -> Result<usize, Error> {
        let read_txn = self.db.begin_read()?;
        let table = match read_txn.open_table(DASHBOARDS_TABLE) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(0),
            Err(e) => return Err(e.into()),
        };
        Ok(table.iter()?.count())
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to create a temporary DashboardStore for tests
    fn create_temp_store() -> Arc<DashboardStore> {
        let temp_dir =
            std::env::temp_dir().join(format!("dashboard_test_{}", uuid::Uuid::new_v4()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();
        let db_path = temp_dir.join("dashboards.redb");
        DashboardStore::open(&db_path).unwrap()
    }

    #[test]
    fn test_dashboard_store() {
        let store = create_temp_store();

        // Create a dashboard
        let mut dashboard = Dashboard::new(
            "Test Dashboard".to_string(),
            DashboardLayout::default_layout(),
        );
        dashboard.id = "test-dashboard".to_string();

        // Save dashboard
        store.save(&dashboard).unwrap();

        // Check exists
        assert!(store.exists("test-dashboard").unwrap());
        assert!(!store.exists("non-existent").unwrap());

        // Load dashboard
        let loaded = store.load("test-dashboard").unwrap();
        assert!(loaded.is_some());
        let loaded = loaded.unwrap();
        assert_eq!(loaded.name, "Test Dashboard");
        assert_eq!(loaded.id, "test-dashboard");

        // List dashboards
        let dashboards = store.list_all().unwrap();
        assert_eq!(dashboards.len(), 1);
        assert_eq!(dashboards[0].id, "test-dashboard");

        // Delete dashboard
        store.delete("test-dashboard").unwrap();
        assert!(!store.exists("test-dashboard").unwrap());
        assert_eq!(store.count().unwrap(), 0);
    }

    #[test]
    fn test_dashboard_with_components() {
        let store = create_temp_store();

        let mut dashboard = Dashboard::new(
            "Component Dashboard".to_string(),
            DashboardLayout::default_layout(),
        );
        dashboard.id = "comp-dashboard".to_string();

        // Add components
        dashboard.components.push(DashboardComponent {
            id: "comp-1".to_string(),
            component_type: "value-card".to_string(),
            position: ComponentPosition {
                x: 0,
                y: 0,
                w: 2,
                h: 1,
                min_w: Some(1),
                min_h: Some(1),
                max_w: None,
                max_h: None,
            },
            title: Some("Test Card".to_string()),
            data_source: None,
            display: None,
            config: None,
            actions: None,
        });

        store.save(&dashboard).unwrap();

        let loaded = store.load("comp-dashboard").unwrap().unwrap();
        assert_eq!(loaded.components.len(), 1);
        assert_eq!(loaded.components[0].id, "comp-1");
        assert_eq!(loaded.components[0].title, Some("Test Card".to_string()));
    }

    #[test]
    fn test_default_dashboard() {
        let store = create_temp_store();

        let mut dashboard1 =
            Dashboard::new("Dashboard 1".to_string(), DashboardLayout::default_layout());
        dashboard1.id = "dash-1".to_string();

        let mut dashboard2 =
            Dashboard::new("Dashboard 2".to_string(), DashboardLayout::default_layout());
        dashboard2.id = "dash-2".to_string();

        store.save(&dashboard1).unwrap();
        store.save(&dashboard2).unwrap();

        // Set dashboard 2 as default
        store.set_default("dash-2").unwrap();

        // Check default ID
        let default_id = store.get_default_id().unwrap();
        assert_eq!(default_id, Some("dash-2".to_string()));

        // Load default
        let default = store.get_default().unwrap();
        assert!(default.is_some());
        assert_eq!(default.unwrap().id, "dash-2");

        // Verify dash-1 is no longer default
        let dash1 = store.load("dash-1").unwrap().unwrap();
        assert_eq!(dash1.is_default, Some(false));

        // Verify dash-2 is default
        let dash2 = store.load("dash-2").unwrap().unwrap();
        assert_eq!(dash2.is_default, Some(true));
    }

    #[test]
    fn test_dashboard_serialization() {
        let dashboard = Dashboard {
            id: "test".to_string(),
            name: "Test Dashboard".to_string(),
            description: None,
            layout: DashboardLayout {
                columns: 12,
                rows: RowsValue::String("auto".to_string()),
                breakpoints: LayoutBreakpoints {
                    lg: 1200,
                    md: 996,
                    sm: 768,
                    xs: 480,
                },
            },
            components: vec![],
            created_at: 12345,
            updated_at: 12346,
            is_default: Some(true),
            sort_order: Some(0),
        };

        let serialized = serde_json::to_string(&dashboard).unwrap();
        let deserialized: Dashboard = serde_json::from_str(&serialized).unwrap();

        assert_eq!(deserialized.id, "test");
        assert_eq!(deserialized.name, "Test Dashboard");
        assert_eq!(deserialized.layout.columns, 12);
        assert_eq!(deserialized.is_default, Some(true));
    }

    #[test]
    fn test_memory_store() {
        let store = DashboardStore::memory().unwrap();

        let mut dashboard =
            Dashboard::new("Memory Test".to_string(), DashboardLayout::default_layout());
        dashboard.id = "mem-test".to_string();

        store.save(&dashboard).unwrap();

        let loaded = store.load("mem-test").unwrap();
        assert!(loaded.is_some());
        assert_eq!(loaded.unwrap().name, "Memory Test");
    }

    #[test]
    fn test_sort_order_and_reorder() {
        let store = create_temp_store();

        // Create three dashboards with explicit orders.
        let mk = |id: &str, order: i32| {
            let mut d = Dashboard::new(format!("D-{}", id), DashboardLayout::default_layout());
            d.id = id.to_string();
            d.sort_order = Some(order);
            store.save(&d).unwrap();
        };
        mk("a", 2);
        mk("b", 0);
        mk("c", 1);

        // max_sort_order returns the largest explicit value.
        assert_eq!(store.max_sort_order().unwrap(), 2);

        // Empty table → 0 (different store, fresh).
        let empty_store = create_temp_store();
        assert_eq!(empty_store.max_sort_order().unwrap(), 0);

        // Batch-reorder: assign new contiguous orders.
        // New desired order: c (top), a, b (bottom).
        store
            .set_sort_orders(&[
                ("c".to_string(), 0),
                ("a".to_string(), 1),
                ("b".to_string(), 2),
            ])
            .unwrap();

        assert_eq!(store.load("c").unwrap().unwrap().sort_order, Some(0));
        assert_eq!(store.load("a").unwrap().unwrap().sort_order, Some(1));
        assert_eq!(store.load("b").unwrap().unwrap().sort_order, Some(2));

        // Reorder with a missing id fails (rolls back the transaction).
        let err = store.set_sort_orders(&[("missing".to_string(), 0)]);
        assert!(err.is_err());
        // Existing data is untouched after rollback.
        assert_eq!(store.load("c").unwrap().unwrap().sort_order, Some(0));

        // Empty input is a no-op success.
        assert!(store.set_sort_orders(&[]).is_ok());
    }
}

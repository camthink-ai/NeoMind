//! AI Agent state.
//!
//! Contains all AI agent-related services:
//! - SessionManager for chat sessions
//! - AgentStore for agent persistence
//! - AgentManager for executing user-defined agents
//! - MarkdownMemoryStore for system-level memory
//! - MemoryScheduler for background memory tasks

use std::sync::Arc;
use tokio::sync::RwLock;

use neomind_agent::memory::MemoryScheduler;
use neomind_agent::SessionManager;
use neomind_storage::{AgentStore, MarkdownMemoryStore, MemoryConfig};

/// AI Agent manager type alias.
pub type AgentManager = Arc<neomind_agent::ai_agent::AiAgentManager>;

/// AI Agent state.
///
/// Provides access to session management, memory, and agent execution.
#[derive(Clone)]
pub struct AgentState {
    /// Session manager for chat sessions.
    pub session_manager: Arc<SessionManager>,

    /// AI Agent store for user-defined automation agents.
    pub agent_store: Arc<AgentStore>,

    /// AI Agent manager for executing user-defined agents (lazy-initialized).
    pub agent_manager: Arc<RwLock<Option<AgentManager>>>,

    /// System memory store for Markdown-based persistent memory.
    pub system_memory_store: Arc<MarkdownMemoryStore>,

    /// Handle to set session ID on the memory tool (shared across sessions).
    pub memory_session_handle: Arc<RwLock<Option<String>>>,

    /// Memory scheduler for background extraction/compression (lazy-initialized).
    pub memory_scheduler: Arc<RwLock<Option<MemoryScheduler>>>,
}

impl AgentState {
    /// Create a new agent state.
    pub fn new(
        session_manager: Arc<SessionManager>,
        agent_store: Arc<AgentStore>,
        agent_manager: Arc<RwLock<Option<AgentManager>>>,
        system_memory_store: Arc<MarkdownMemoryStore>,
    ) -> Self {
        Self {
            session_manager,
            agent_store,
            agent_manager,
            system_memory_store,
            memory_session_handle: Arc::new(RwLock::new(None)),
            memory_scheduler: Arc::new(RwLock::new(None)),
        }
    }

    /// Start the memory scheduler (temp file cleanup).
    /// Idempotent: if a scheduler is already running, returns Ok without creating a duplicate.
    pub async fn start_memory_scheduler(&self) -> Result<(), String> {
        // Idempotency check — avoid spawning duplicate background tasks
        {
            let guard = self.memory_scheduler.read().await;
            if guard.is_some() {
                tracing::debug!("Memory scheduler already running, skipping");
                return Ok(());
            }
        }

        let config = MemoryConfig::load();

        if !config.enabled {
            tracing::info!("Memory system disabled, not starting scheduler");
            return Ok(());
        }

        let store = Arc::new(RwLock::new((*self.system_memory_store).clone()));

        let mut scheduler = MemoryScheduler::new(store, config);

        scheduler.start();

        let mut scheduler_guard = self.memory_scheduler.write().await;
        *scheduler_guard = Some(scheduler);

        tracing::info!("Memory scheduler started successfully");
        Ok(())
    }

    /// Stop the memory scheduler
    pub async fn stop_memory_scheduler(&self) {
        let mut scheduler_guard = self.memory_scheduler.write().await;
        if let Some(mut scheduler) = scheduler_guard.take() {
            scheduler.stop();
            tracing::info!("Memory scheduler stopped");
        }
    }

    /// Create a minimal agent state (for testing).
    #[cfg(test)]
    pub fn minimal() -> Self {
        Self {
            session_manager: Arc::new(SessionManager::memory()),
            agent_store: AgentStore::memory().unwrap(),
            agent_manager: Arc::new(RwLock::new(None)),
            system_memory_store: Arc::new(MarkdownMemoryStore::with_config(
                std::env::temp_dir().join("test-memory"),
                neomind_storage::MemoryConfig::default(),
            )),
            memory_session_handle: Arc::new(RwLock::new(None)),
            memory_scheduler: Arc::new(RwLock::new(None)),
        }
    }
}

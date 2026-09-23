//! Handler tests module.

pub mod agents;
pub mod auth_users;
pub mod basic;
// llm_backends.rs deprecated - API changed significantly
pub mod devices;
pub mod extensions;
// events.rs and events_sse.rs removed due to axum 0.7 API incompatibility
// memory.rs and tools.rs removed - functionality moved to neomind-agent
// plugins.rs deprecated - migrated to Extension system
pub mod messages_chain;
pub mod rules;
pub mod sessions;
// settings.rs deprecated - API changed significantly

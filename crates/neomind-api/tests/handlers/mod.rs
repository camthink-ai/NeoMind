//! Handler tests module.

pub mod auth_users;
pub mod basic;
// llm_backends.rs deprecated - API changed significantly
pub mod devices;
pub mod extensions;
// events.rs and events_sse.rs removed due to axum 0.7 API incompatibility
pub mod memory;
// plugins.rs deprecated - migrated to Extension system
pub mod rules;
pub mod sessions;
// settings.rs deprecated - API changed significantly
pub mod tools;

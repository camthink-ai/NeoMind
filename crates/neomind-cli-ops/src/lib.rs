pub mod api_client;
pub mod auth_cmd;
pub mod auto_auth;
pub mod dispatch;
pub mod kv;
pub mod output;
pub mod types;
pub mod user_cmd;

// Command modules
pub mod agent_cmd;
pub mod config_cmd;
pub mod connector;
pub mod dashboard;
pub mod data_push;
pub mod device;
pub mod extension;
pub mod llm;
pub mod message;
pub mod rule;
pub mod settings;
pub mod system;
pub mod transform;
pub mod widget;

pub use api_client::ApiClient;

//! OpenAPI documentation for the NeoTalk API.
//!
//! This module sets up the OpenAPI/Swagger documentation for all API endpoints.
//! The documentation is available at:
//! - `/api-docs` - Swagger UI
//! - `/api/openapi.json` - OpenAPI JSON schema

use utoipa::{Modify, OpenApi};
use utoipa_swagger_ui::SwaggerUi;

/// Custom modifier to add server information to the OpenAPI spec.
#[derive(Default)]
struct ServerModifier;

impl Modify for ServerModifier {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        // Update the info section
        openapi.info.title = "NeoTalk API".to_string();
        openapi.info.version = "0.1.0".to_string();
        openapi.info.description = Some(
            "NeoTalk Edge AI Agent API\n\n\
             ## Overview\n\n\
             NeoTalk is an edge-deployed AI agent system with multi-backend LLM support,\n\
             device management, rule engine, and workflow automation capabilities.\n\n\
             ## Authentication\n\n\
             Most endpoints do not require authentication. For protected endpoints,\n\
             use HTTP Bearer token authentication.\n\n\
             ## WebSocket\n\n\
             Real-time communication is available via WebSocket at `/api/chat`."
            .to_string()
        );
    }
}

/// NeoTalk OpenAPI documentation.
#[derive(OpenApi)]
#[openapi(
    modifiers(&ServerModifier),
    tags(
        (name = "documentation", description = "API documentation"),
        (name = "devices", description = "Device management"),
        (name = "commands", description = "Command history"),
        (name = "decisions", description = "AI decision management"),
        (name = "stats", description = "System statistics"),
        (name = "sessions", description = "Chat sessions"),
        (name = "settings", description = "System settings"),
        (name = "alerts", description = "Alert management"),
        (name = "automation", description = "Automation and workflows"),
    )
)]
pub struct ApiDoc;

/// Create the Swagger UI router for nesting.
pub fn swagger_ui() -> utoipa_swagger_ui::SwaggerUi {
    SwaggerUi::new("/api-docs{/spec}")
        .url("/api/openapi.json", ApiDoc::openapi())
}

/// Handler that returns the OpenAPI JSON schema.
pub async fn openapi_json_handler() -> impl axum::response::IntoResponse {
    let json = ApiDoc::openapi()
        .to_pretty_json()
        .expect("Failed to serialize OpenAPI spec");

    (
        [(axum::http::header::CONTENT_TYPE, "application/json;charset=utf-8")],
        json,
    )
}

/// Get the OpenAPI spec as JSON.
pub fn openapi_json() -> String {
    ApiDoc::openapi()
        .to_pretty_json()
        .expect("Failed to serialize OpenAPI spec")
}

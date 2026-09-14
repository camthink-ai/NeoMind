//! OpenAPI schema generation via utoipa.
//!
//! First batch: auth (login/register/keys) + devices (CRUD + telemetry) —
//! the domains third parties integrate against. The spec is served at
//! `GET /api/docs/openapi.json` and rendered by the Scalar console at
//! `/api/docs`. Routes outside the annotated set remain discoverable via
//! `/api/docs/routes.json` (the CI-enforced route index).
//!
//! Adding an endpoint: derive `utoipa::path` on the handler (params,
//! request body, responses with the shapes from the handler's DTOs), add it
//! to `paths()` below, and extend the drift test if it's a new router.

use utoipa::OpenApi;

/// The NeoMind OpenAPI document. `info` describes the service; `paths` is
/// hand-maintained alongside the handlers it documents — the api_docs drift
/// test fails when an annotated path is not actually routed.
#[derive(OpenApi)]
#[openapi(
    info(
        title = "NeoMind API",
        version = "0.9.24",
        description = "Edge AI/IoT platform API. All responses use the \
{success, data|error:{code,message}} envelope. Timestamps are Unix seconds \
unless a field is documented otherwise. Interactive docs: /api/docs; full \
route index (including not-yet-schematized endpoints): /api/docs/routes.json."
    ),
    paths(
        // ---- Auth ----
        crate::handlers::auth_users::login_handler,
        crate::handlers::auth_users::register_handler,
        crate::handlers::auth::create_key_handler,
        crate::handlers::auth::list_keys_handler,
        // ---- Devices ----
        crate::handlers::devices::crud::list_devices_handler,
        crate::handlers::devices::crud::add_device_handler,
        crate::handlers::devices::crud::get_device_handler,
        crate::handlers::devices::crud::update_device_handler,
        crate::handlers::devices::crud::delete_device_handler,
        crate::handlers::devices::crud::get_device_current_handler,
        // ---- Telemetry ----
        crate::handlers::devices::telemetry::get_device_telemetry_handler,
        crate::handlers::devices::telemetry::get_device_telemetry_summary_handler,
        crate::handlers::devices::metrics::write_metric_handler,
        crate::handlers::devices::metrics::send_command_handler,
    ),
    tags(
        (name = "auth", description = "Login, registration, API keys"),
        (name = "devices", description = "Device CRUD and current values"),
        (name = "telemetry", description = "Time-series query, write, and commands"),
    )
)]
pub struct ApiDoc;

/// GET /api/docs/openapi.json — the machine-readable OpenAPI 3 document.
pub async fn openapi_json_handler() -> axum::Json<utoipa::openapi::OpenApi> {
    axum::Json(ApiDoc::openapi())
}

//! Notification-channel access control.
//!
//! A channel's config carries live credentials — the SMTP password, the bot
//! token, the webhook key — and its recipients are real mail addresses. The
//! auth middleware only *authenticates* (it does not authorize), so these
//! tests pin the handler-level role check: a non-admin must not read or
//! change a channel, while still being able to see *which* channels exist,
//! which is all the agent editor's notify picker needs.

use axum::extract::{Extension, Path, State};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

use neomind_api::auth_users::{SessionInfo, UserRole};
use neomind_api::handlers::message_channels::{
    create_channel_handler, get_channel_handler, list_channels_handler, CreateChannelRequest,
};
use neomind_api::handlers::ServerState;

async fn create_test_server_state() -> ServerState {
    crate::common::create_test_server_state().await
}

fn session(role: UserRole) -> SessionInfo {
    SessionInfo {
        user_id: "u-1".to_string(),
        username: "someone".to_string(),
        role,
        created_at: 0,
        expires_at: i64::MAX,
    }
}

const WEBHOOK_SECRET: &str = "Bearer tok_live_supersecret";

/// Create a channel the way the UI does, and return its name.
async fn create_channel_with_secret(state: &ServerState) -> String {
    let name = "ops-webhook".to_string();
    let created = create_channel_handler(
        State(state.clone()),
        Extension(session(UserRole::Admin)),
        Json(CreateChannelRequest {
            name: name.clone(),
            channel_type: "webhook".to_string(),
            config: json!({
                "url": "https://example.com/hook",
                "headers": { "Authorization": WEBHOOK_SECRET },
            }),
        }),
    )
    .await
    .expect("an admin must be able to create a channel");
    assert!(created.0.success);
    name
}

async fn status_of(result: Result<Response, neomind_api::models::ErrorResponse>) -> u16 {
    match result {
        Ok(_) => 200,
        Err(e) => e.into_response().status().as_u16(),
    }
}

/// The channel list is reachable from pages that are not channel management,
/// so a non-admin still gets names/types/enabled — but the config (credentials)
/// and the recipient list must not be in the payload at all.
#[tokio::test]
async fn a_non_admin_lists_channels_without_their_credentials() {
    let state = create_test_server_state().await;
    let name = create_channel_with_secret(&state).await;

    let listed = list_channels_handler(State(state.clone()), Extension(session(UserRole::User)))
        .await
        .expect("a non-admin may still see which channels exist");
    let body = listed.0.data.expect("the envelope carries data");
    let entry = body["channels"]
        .as_array()
        .expect("channels array")
        .iter()
        .find(|c| c["name"] == json!(name))
        .expect("the channel must be listed");

    assert_eq!(entry["channel_type"], json!("webhook"));
    assert!(
        entry.get("config").is_none(),
        "a non-admin must not receive the channel config: {entry}"
    );
    assert!(
        entry.get("recipients").is_none(),
        "a non-admin must not receive the recipient list: {entry}"
    );

    // The same call as an admin carries the config, which is what the edit
    // dialog prefills from — proving the omission above is the role boundary
    // and not a field the API stopped returning.
    let as_admin = list_channels_handler(State(state), Extension(session(UserRole::Admin)))
        .await
        .expect("admin list");
    let admin_body = as_admin.0.data.expect("data");
    let admin_entry = admin_body["channels"]
        .as_array()
        .expect("channels array")
        .iter()
        .find(|c| c["name"] == json!(name))
        .expect("the channel must be listed");
    assert_eq!(
        admin_entry["config"]["headers"]["Authorization"],
        json!(WEBHOOK_SECRET),
        "an admin still needs the config to edit the channel"
    );
}

/// Reading a single channel returns its config verbatim, so it is admin-only
/// (403, not 404 — the caller is allowed to know the platform exists).
#[tokio::test]
async fn reading_one_channel_is_admin_only() {
    let state = create_test_server_state().await;
    let name = create_channel_with_secret(&state).await;

    let denied = status_of(
        get_channel_handler(
            State(state.clone()),
            Path(name.clone()),
            Extension(session(UserRole::User)),
        )
        .await
        .map(|_| ().into_response()),
    )
    .await;
    assert_eq!(denied, 403, "a non-admin must not read a channel's config");

    let allowed = get_channel_handler(
        State(state),
        Path(name),
        Extension(session(UserRole::Admin)),
    )
    .await;
    assert!(allowed.is_ok(), "an admin reads the channel");
}

/// Writing is the other half: whoever can set a channel's config decides where
/// the platform's alerts get delivered, so a non-admin must not reach it.
#[tokio::test]
async fn creating_a_channel_is_admin_only() {
    let state = create_test_server_state().await;

    let denied = status_of(
        create_channel_handler(
            State(state),
            Extension(session(UserRole::User)),
            Json(CreateChannelRequest {
                name: "smuggled".to_string(),
                channel_type: "webhook".to_string(),
                config: json!({ "url": "https://attacker.example/collect" }),
            }),
        )
        .await
        .map(|_| ().into_response()),
    )
    .await;
    assert_eq!(denied, 403, "a non-admin must not create a channel");
}

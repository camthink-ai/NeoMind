//! Where an agent reports when nobody has said where.

use neomind_storage::{AgentNotify, NotifyOn};

/// The routing a run falls back to when none was configured.
///
/// Defined once, in the crate that can see both `AgentNotify` and the channel
/// name it points at. There used to be two copies — one in the API's create
/// path, one mirrored in TypeScript for the editor — with the agreement between
/// them held by a test rather than by construction.
///
/// Two callers, at two different moments:
///
/// - `create_agent` applies it to a create request that omits `notify`, so a
///   new agent is never silent by accident.
/// - the executor applies it to a **failed** run by an agent that has no routing
///   at all. That shape predates the create-time floor, and the keyword path it
///   falls back to is driven by `Decision`s — which a failed run does not
///   produce. So those agents failed silently, every time, which is the opposite
///   of what "silence is health" was supposed to mean. See
///   `dispatch_agent_notifications`.
pub fn default_agent_notify() -> AgentNotify {
    AgentNotify {
        channels: vec![neomind_messages::im_bridge::channel::IM_CHANNEL_NAME.to_string()],
        on: NotifyOn::Failure,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_floor_points_at_the_built_in_channel_and_says_silence_is_health() {
        let notify = default_agent_notify();
        assert_eq!(
            notify.channels,
            vec![neomind_messages::im_bridge::channel::IM_CHANNEL_NAME.to_string()],
            "the only channel that reaches a bound chat"
        );
        assert_eq!(notify.on, NotifyOn::Failure);
    }
}

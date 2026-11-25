//! Create-path defaults that used to live in `neomind_agent::ai_agent`.

use neomind_storage::{AgentNotify, NotifyOn};

/// Routing applied when a create request omits `notify`.
///
/// `IM` reaches whatever chats the operator bound under Settings → IM Bridges.
/// `on: Failure` keeps silence-as-health: only failed runs notify by default.
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
    fn floor_points_at_im_and_silence_is_health() {
        let notify = default_agent_notify();
        assert_eq!(
            notify.channels,
            vec![neomind_messages::im_bridge::channel::IM_CHANNEL_NAME.to_string()]
        );
        assert_eq!(notify.on, NotifyOn::Failure);
    }
}

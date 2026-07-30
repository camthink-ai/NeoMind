use neomind_core::event::NeoMindEvent;

#[test]
fn im_message_received_roundtrips() {
    let e = NeoMindEvent::ImMessageReceived {
        platform: "telegram".into(),
        im_chat_id: "123".into(),
        sender_id: "42".into(),
        text: "hi".into(),
        msg_id: "u1".into(),
        timestamp: 1,
    };
    let s = serde_json::to_string(&e).unwrap();
    assert!(s.contains("\"type\":\"ImMessageReceived\""));
    let back: NeoMindEvent = serde_json::from_str(&s).unwrap();
    assert!(matches!(back, NeoMindEvent::ImMessageReceived { .. }));
}

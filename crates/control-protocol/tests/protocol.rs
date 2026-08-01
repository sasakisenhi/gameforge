use gameforge_control_protocol::{
    ClientHello, CommandEnvelope, ProtocolError, ProtocolVersion, negotiate,
};

#[test]
fn accepts_only_the_supported_protocol_version() {
    let compatible = ClientHello::new("desktop-1", ProtocolVersion::CURRENT).unwrap();
    assert!(negotiate(&compatible).is_ok());

    let incompatible = ClientHello::new("old-cli", ProtocolVersion::new(999)).unwrap();
    assert!(matches!(
        negotiate(&incompatible),
        Err(ProtocolError::IncompatibleVersion { .. })
    ));
}

#[test]
fn command_envelope_requires_stable_non_empty_ids() {
    let envelope = CommandEnvelope::new(
        "CMD-1",
        "PROJECT-1",
        "CLI-1",
        4,
        "2026-08-01T12:00:00+09:00",
        "queue-task-run",
    )
    .unwrap();
    assert_eq!(envelope.expected_aggregate_version(), 4);
    assert_eq!(*envelope.payload(), "queue-task-run");

    assert!(matches!(
        CommandEnvelope::new("", "PROJECT-1", "CLI-1", 4, "now", ()),
        Err(ProtocolError::EmptyField("command_id"))
    ));
}

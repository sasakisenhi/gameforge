use gameforge_control_protocol::{
    ClientHello, CommandEnvelope, ProtocolError, ProtocolVersion, ServerHello, negotiate,
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
fn protocol_version_json_format_is_stable() {
    let json = ProtocolVersion::CURRENT.encode_json().unwrap();
    assert_eq!(json, "1");
    assert_eq!(
        ProtocolVersion::decode_json(&json).unwrap(),
        ProtocolVersion::CURRENT
    );
}

#[test]
fn hello_json_formats_are_stable_and_round_trip() {
    let client = ClientHello::new("desktop-1", ProtocolVersion::CURRENT).unwrap();
    let client_json = client.encode_json().unwrap();
    assert_eq!(
        client_json,
        r#"{"client_id":"desktop-1","protocol_version":1}"#
    );
    assert_eq!(ClientHello::decode_json(&client_json).unwrap(), client);

    let server = negotiate(&client).unwrap();
    let server_json = server.encode_json().unwrap();
    assert_eq!(server_json, r#"{"protocol_version":1}"#);
    assert_eq!(ServerHello::decode_json(&server_json).unwrap(), server);
}

#[test]
fn hello_decode_rejects_invalid_wire_values_explicitly() {
    assert!(matches!(
        ClientHello::decode_json(r#"{"client_id":" ","protocol_version":1}"#),
        Err(ProtocolError::EmptyField("client_id"))
    ));
    assert!(matches!(
        ClientHello::decode_json(
            r#"{"client_id":"desktop-1","protocol_version":1,"extra":true}"#
        ),
        Err(ProtocolError::UnknownField(field)) if field == "extra"
    ));
    assert!(matches!(
        ClientHello::decode_json(r#"{"client_id":"desktop-1","protocol_version":999}"#),
        Err(ProtocolError::IncompatibleVersion { .. })
    ));
    assert!(matches!(
        ClientHello::decode_json("{not-json}"),
        Err(ProtocolError::MalformedJson(_))
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

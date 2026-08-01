//! Local client/coordinator protocol types.
#![allow(clippy::missing_errors_doc)]

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireClientHello {
    client_id: String,
    protocol_version: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireServerHello {
    protocol_version: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ProtocolVersion(u32);

impl ProtocolVersion {
    pub const CURRENT: Self = Self(1);

    #[must_use]
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }

    pub fn encode_json(self) -> Result<String, ProtocolError> {
        require_current_version(self)?;
        encode_json(&self)
    }

    pub fn decode_json(json: &str) -> Result<Self, ProtocolError> {
        let value = parse_json(json)?;
        version_from_value(&value)
    }
}

impl Serialize for ProtocolVersion {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u32(self.0)
    }
}

impl<'de> Deserialize<'de> for ProtocolVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let version = Self(u32::deserialize(deserializer)?);
        require_current_version(version).map_err(serde::de::Error::custom)?;
        Ok(version)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ClientHello {
    client_id: String,
    protocol_version: ProtocolVersion,
}

impl ClientHello {
    pub fn new(
        client_id: impl Into<String>,
        protocol_version: ProtocolVersion,
    ) -> Result<Self, ProtocolError> {
        let client_id = client_id.into();
        Ok(Self {
            client_id: required("client_id", &client_id)?,
            protocol_version,
        })
    }

    #[must_use]
    pub fn client_id(&self) -> &str {
        &self.client_id
    }

    #[must_use]
    pub const fn protocol_version(&self) -> ProtocolVersion {
        self.protocol_version
    }

    pub fn encode_json(&self) -> Result<String, ProtocolError> {
        require_current_version(self.protocol_version)?;
        encode_json(self)
    }

    pub fn decode_json(json: &str) -> Result<Self, ProtocolError> {
        let value = parse_json(json)?;
        validate_object(&value, &["client_id", "protocol_version"])?;
        validate_version_field(&value)?;
        let wire: WireClientHello = decode_json(json)?;
        Self::new(wire.client_id, ProtocolVersion::new(wire.protocol_version))
    }
}

impl<'de> Deserialize<'de> for ClientHello {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = WireClientHello::deserialize(deserializer)?;
        let protocol_version = ProtocolVersion::new(wire.protocol_version);
        require_current_version(protocol_version).map_err(serde::de::Error::custom)?;
        Self::new(wire.client_id, protocol_version).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ServerHello {
    protocol_version: ProtocolVersion,
}

impl ServerHello {
    #[must_use]
    pub const fn protocol_version(&self) -> ProtocolVersion {
        self.protocol_version
    }

    pub fn encode_json(&self) -> Result<String, ProtocolError> {
        require_current_version(self.protocol_version)?;
        encode_json(self)
    }

    pub fn decode_json(json: &str) -> Result<Self, ProtocolError> {
        let value = parse_json(json)?;
        validate_object(&value, &["protocol_version"])?;
        validate_version_field(&value)?;
        let wire: WireServerHello = decode_json(json)?;
        Ok(Self {
            protocol_version: ProtocolVersion::new(wire.protocol_version),
        })
    }
}

impl<'de> Deserialize<'de> for ServerHello {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = WireServerHello::deserialize(deserializer)?;
        let protocol_version = ProtocolVersion::new(wire.protocol_version);
        require_current_version(protocol_version).map_err(serde::de::Error::custom)?;
        Ok(Self { protocol_version })
    }
}

pub fn negotiate(client: &ClientHello) -> Result<ServerHello, ProtocolError> {
    if client.protocol_version != ProtocolVersion::CURRENT {
        return Err(ProtocolError::IncompatibleVersion {
            expected: ProtocolVersion::CURRENT,
            actual: client.protocol_version,
        });
    }
    Ok(ServerHello {
        protocol_version: ProtocolVersion::CURRENT,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandEnvelope<T> {
    command_id: String,
    project_id: String,
    client_id: String,
    expected_aggregate_version: u64,
    issued_at: String,
    payload: T,
}

impl<T> CommandEnvelope<T> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        command_id: impl Into<String>,
        project_id: impl Into<String>,
        client_id: impl Into<String>,
        expected_aggregate_version: u64,
        issued_at: impl Into<String>,
        payload: T,
    ) -> Result<Self, ProtocolError> {
        let command_id = command_id.into();
        let project_id = project_id.into();
        let client_id = client_id.into();
        let issued_at = issued_at.into();
        Ok(Self {
            command_id: required("command_id", &command_id)?,
            project_id: required("project_id", &project_id)?,
            client_id: required("client_id", &client_id)?,
            expected_aggregate_version,
            issued_at: required("issued_at", &issued_at)?,
            payload,
        })
    }

    #[must_use]
    pub fn command_id(&self) -> &str {
        &self.command_id
    }

    #[must_use]
    pub fn project_id(&self) -> &str {
        &self.project_id
    }

    #[must_use]
    pub fn client_id(&self) -> &str {
        &self.client_id
    }

    #[must_use]
    pub const fn expected_aggregate_version(&self) -> u64 {
        self.expected_aggregate_version
    }

    #[must_use]
    pub fn issued_at(&self) -> &str {
        &self.issued_at
    }

    #[must_use]
    pub const fn payload(&self) -> &T {
        &self.payload
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolError {
    EmptyField(&'static str),
    IncompatibleVersion {
        expected: ProtocolVersion,
        actual: ProtocolVersion,
    },
    MalformedJson(String),
    UnknownField(String),
    InvalidMessage(String),
    JsonEncoding(String),
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyField(field) => write!(formatter, "{field} must not be empty"),
            Self::IncompatibleVersion { expected, actual } => write!(
                formatter,
                "incompatible protocol version: expected {}, got {}",
                expected.get(),
                actual.get()
            ),
            Self::MalformedJson(reason) => write!(formatter, "malformed JSON: {reason}"),
            Self::UnknownField(field) => write!(formatter, "unknown field: {field}"),
            Self::InvalidMessage(reason) => write!(formatter, "invalid protocol message: {reason}"),
            Self::JsonEncoding(reason) => write!(formatter, "JSON encoding failed: {reason}"),
        }
    }
}

impl std::error::Error for ProtocolError {}

fn required(field: &'static str, value: &str) -> Result<String, ProtocolError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(ProtocolError::EmptyField(field));
    }
    Ok(trimmed.to_owned())
}

fn require_current_version(version: ProtocolVersion) -> Result<(), ProtocolError> {
    if version != ProtocolVersion::CURRENT {
        return Err(ProtocolError::IncompatibleVersion {
            expected: ProtocolVersion::CURRENT,
            actual: version,
        });
    }
    Ok(())
}

fn version_from_value(value: &Value) -> Result<ProtocolVersion, ProtocolError> {
    let raw = value.as_u64().ok_or_else(|| {
        ProtocolError::InvalidMessage("protocol version must be an unsigned integer".to_owned())
    })?;
    let raw = u32::try_from(raw).map_err(|_| {
        ProtocolError::InvalidMessage("protocol version is outside the u32 range".to_owned())
    })?;
    let version = ProtocolVersion::new(raw);
    require_current_version(version)?;
    Ok(version)
}

fn validate_version_field(value: &Value) -> Result<(), ProtocolError> {
    let version = value
        .as_object()
        .and_then(|object| object.get("protocol_version"));
    if let Some(version) = version {
        version_from_value(version)?;
    }
    Ok(())
}

fn parse_json(json: &str) -> Result<Value, ProtocolError> {
    serde_json::from_str(json).map_err(|error| ProtocolError::MalformedJson(error.to_string()))
}

fn validate_object(value: &Value, allowed_fields: &[&str]) -> Result<(), ProtocolError> {
    let object = value
        .as_object()
        .ok_or_else(|| ProtocolError::InvalidMessage("expected a JSON object".to_owned()))?;
    if let Some(field) = object
        .keys()
        .find(|field| !allowed_fields.contains(&field.as_str()))
    {
        return Err(ProtocolError::UnknownField(field.clone()));
    }
    Ok(())
}

fn encode_json<T: Serialize>(value: &T) -> Result<String, ProtocolError> {
    serde_json::to_string(value).map_err(|error| ProtocolError::JsonEncoding(error.to_string()))
}

fn decode_json<T>(json: &str) -> Result<T, ProtocolError>
where
    T: for<'de> Deserialize<'de>,
{
    serde_json::from_str(json).map_err(|error| ProtocolError::InvalidMessage(error.to_string()))
}

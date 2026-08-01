//! Local client/coordinator protocol types.
#![allow(clippy::missing_errors_doc)]

use std::fmt;

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
}

#[derive(Debug, Clone, PartialEq, Eq)]
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerHello {
    protocol_version: ProtocolVersion,
}

impl ServerHello {
    #[must_use]
    pub const fn protocol_version(&self) -> ProtocolVersion {
        self.protocol_version
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

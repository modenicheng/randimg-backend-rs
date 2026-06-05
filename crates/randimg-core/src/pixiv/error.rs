use std::fmt;

/// Error type for Pixiv module operations.
///
/// Wraps the underlying `pixiv_client::PixivError` and adds contextual
/// information such as the credential ID and operation name.
#[derive(Debug)]
pub enum PixivError {
    /// Authentication failed (invalid token, expired refresh token, etc.)
    Auth {
        message: String,
        credential_id: Option<i32>,
        source: Option<pixiv_client::PixivError>,
    },

    /// API request failed (network error, timeout, etc.)
    Request {
        message: String,
        operation: String,
        source: pixiv_client::PixivError,
    },

    /// API returned an error status code
    Status {
        status: reqwest::StatusCode,
        operation: String,
        source: pixiv_client::PixivError,
    },

    /// Token persistence failed (database error)
    Persistence {
        message: String,
        credential_id: i32,
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// No active credentials found
    NoCredentials,

    /// Invalid configuration or parameters
    InvalidConfig(String),
}

impl fmt::Display for PixivError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PixivError::Auth {
                message,
                credential_id,
                ..
            } => {
                if let Some(id) = credential_id {
                    write!(f, "Pixiv auth failed (credential {}): {}", id, message)
                } else {
                    write!(f, "Pixiv auth failed: {}", message)
                }
            }
            PixivError::Request {
                message,
                operation,
                ..
            } => {
                write!(f, "Pixiv {} request failed: {}", operation, message)
            }
            PixivError::Status {
                status,
                operation,
                ..
            } => {
                write!(
                    f,
                    "Pixiv {} returned error status: {}",
                    operation, status
                )
            }
            PixivError::Persistence {
                message,
                credential_id,
                ..
            } => {
                write!(
                    f,
                    "Failed to persist tokens for credential {}: {}",
                    credential_id, message
                )
            }
            PixivError::NoCredentials => {
                write!(f, "No active Pixiv credentials found")
            }
            PixivError::InvalidConfig(msg) => {
                write!(f, "Invalid Pixiv configuration: {}", msg)
            }
        }
    }
}

impl std::error::Error for PixivError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            PixivError::Auth { source, .. } => source.as_ref().map(|e| e as &dyn std::error::Error),
            PixivError::Request { source, .. } => Some(source),
            PixivError::Status { source, .. } => Some(source),
            PixivError::Persistence { source, .. } => Some(source.as_ref()),
            _ => None,
        }
    }
}

impl PixivError {
    /// Create an auth error with credential context
    pub fn auth_credential(message: impl Into<String>, credential_id: i32) -> Self {
        PixivError::Auth {
            message: message.into(),
            credential_id: Some(credential_id),
            source: None,
        }
    }

    /// Create an auth error from a source error
    pub fn auth_from_source(
        source: pixiv_client::PixivError,
        credential_id: Option<i32>,
    ) -> Self {
        PixivError::Auth {
            message: source.to_string(),
            credential_id,
            source: Some(source),
        }
    }

    /// Create a request error with operation context
    pub fn request(operation: impl Into<String>, source: pixiv_client::PixivError) -> Self {
        let operation = operation.into();
        PixivError::Request {
            message: source.to_string(),
            operation,
            source,
        }
    }

    /// Create a status error with operation context
    pub fn status(
        status: reqwest::StatusCode,
        operation: impl Into<String>,
        source: pixiv_client::PixivError,
    ) -> Self {
        PixivError::Status {
            status,
            operation: operation.into(),
            source,
        }
    }

    /// Create a persistence error
    pub fn persistence(
        message: impl Into<String>,
        credential_id: i32,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        PixivError::Persistence {
            message: message.into(),
            credential_id,
            source: Box::new(source),
        }
    }

    /// Returns true if this is an authentication error
    pub fn is_auth_error(&self) -> bool {
        matches!(self, PixivError::Auth { .. })
    }

    /// Returns the credential ID if available
    pub fn credential_id(&self) -> Option<i32> {
        match self {
            PixivError::Auth { credential_id, .. } => *credential_id,
            PixivError::Persistence { credential_id, .. } => Some(*credential_id),
            _ => None,
        }
    }

    /// Returns the operation name if available
    pub fn operation(&self) -> Option<&str> {
        match self {
            PixivError::Request { operation, .. } => Some(operation),
            PixivError::Status { operation, .. } => Some(operation),
            _ => None,
        }
    }
}

/// Convert from String errors (for backward compatibility)
impl From<String> for PixivError {
    fn from(s: String) -> Self {
        PixivError::InvalidConfig(s)
    }
}

/// Convert from pixiv_client::PixivError
impl From<pixiv_client::PixivError> for PixivError {
    fn from(err: pixiv_client::PixivError) -> Self {
        match &err {
            pixiv_client::PixivError::Auth(_) => PixivError::auth_from_source(err, None),
            pixiv_client::PixivError::Status(status) => {
                PixivError::status(*status, "unknown", err)
            }
            _ => PixivError::request("unknown", err),
        }
    }
}

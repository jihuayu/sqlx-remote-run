use serde::Serialize;

use crate::sql_kind::StatementKind;
use crate::Permission;

/// Stable machine-readable error codes.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteSqlErrorCode {
    /// Authentication failed.
    Unauthorized,
    /// Authenticated client is not allowed to use this endpoint.
    Forbidden,
    /// Request shape, SQL shape, or parameter data is invalid.
    InvalidRequest,
    /// The configured permission does not allow this statement.
    PermissionDenied,
    /// A configured execution limit was exceeded.
    LimitExceeded,
    /// SQLite or SQLx rejected the statement at execution time.
    SqlExecution,
    /// The statement exceeded the configured timeout.
    Timeout,
}

/// JSON error response shape used by framework adapters.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ErrorResponse {
    /// Stable machine-readable code.
    pub code: RemoteSqlErrorCode,
    /// Human-readable error message.
    pub message: String,
}

impl ErrorResponse {
    /// Builds a JSON-safe response body from an error.
    #[must_use]
    pub fn from_error(error: &RemoteSqlError) -> Self {
        Self {
            code: error.code(),
            message: error.to_string(),
        }
    }
}

/// Errors returned by the remote SQL core service.
#[derive(Debug, thiserror::Error)]
pub enum RemoteSqlError {
    /// Missing or invalid bearer token.
    #[error("missing or invalid bearer token")]
    Unauthorized,
    /// Peer IP is not allowed.
    #[error("peer IP is not allowed")]
    ForbiddenIp,
    /// Peer IP is required because an allowlist is configured.
    #[error("peer IP is required when IP allowlist is configured")]
    MissingPeerIp,
    /// The SQL text is empty.
    #[error("SQL must not be empty")]
    EmptySql,
    /// The SQL parser rejected the request.
    #[error("invalid SQL: {0}")]
    InvalidSql(String),
    /// The SQL request contained anything other than exactly one statement.
    #[error("exactly one SQL statement is required")]
    ExpectedSingleStatement,
    /// Parsed SQL did not map to a supported statement class.
    #[error("unsupported SQL statement")]
    UnsupportedStatement,
    /// Configured permission does not allow the parsed statement.
    #[error("permission {permission:?} does not allow {statement:?} statements")]
    PermissionDenied {
        /// Configured permission.
        permission: Permission,
        /// Parsed statement kind.
        statement: StatementKind,
    },
    /// SQL text exceeded configured byte limit.
    #[error("SQL exceeded max_sql_bytes limit of {max}")]
    SqlTooLarge {
        /// Configured maximum size.
        max: usize,
    },
    /// Parameter count exceeded configured limit.
    #[error("parameter count exceeded max_params limit of {max}")]
    TooManyParams {
        /// Configured maximum parameter count.
        max: usize,
    },
    /// Row count exceeded configured limit.
    #[error("row count exceeded max_rows limit of {max}")]
    TooManyRows {
        /// Configured maximum row count.
        max: usize,
    },
    /// A blob parameter could not be decoded.
    #[error("blob parameter is not valid base64")]
    InvalidBlobBase64 {
        /// Source decode error.
        #[source]
        source: base64::DecodeError,
    },
    /// SQLite execution failed.
    #[error("SQLite execution failed: {0}")]
    SqlExecution(#[from] sqlx::Error),
    /// The request exceeded the configured timeout.
    #[error("SQL execution timed out")]
    Timeout,
}

impl RemoteSqlError {
    /// Returns the stable error code for this error.
    #[must_use]
    pub fn code(&self) -> RemoteSqlErrorCode {
        match self {
            Self::Unauthorized => RemoteSqlErrorCode::Unauthorized,
            Self::ForbiddenIp | Self::MissingPeerIp => RemoteSqlErrorCode::Forbidden,
            Self::EmptySql
            | Self::InvalidSql(_)
            | Self::ExpectedSingleStatement
            | Self::UnsupportedStatement
            | Self::InvalidBlobBase64 { .. } => RemoteSqlErrorCode::InvalidRequest,
            Self::PermissionDenied { .. } => RemoteSqlErrorCode::PermissionDenied,
            Self::SqlTooLarge { .. } | Self::TooManyParams { .. } | Self::TooManyRows { .. } => {
                RemoteSqlErrorCode::LimitExceeded
            }
            Self::SqlExecution(_) => RemoteSqlErrorCode::SqlExecution,
            Self::Timeout => RemoteSqlErrorCode::Timeout,
        }
    }
}

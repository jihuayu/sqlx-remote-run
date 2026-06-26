//! Controlled remote SQL execution for local SQLx SQLite pools.
//!
//! `sqlx_remote_run` exposes a small JSON protocol and a framework-neutral
//! service that can execute a single SQLite statement against a local
//! [`sqlx::SqlitePool`] after token, IP, limit, and permission checks.
//!
//! The optional `axum` feature adds an Axum router adapter.

mod auth;
mod config;
mod error;
mod protocol;
mod service;
mod sql_kind;

#[cfg(feature = "axum")]
pub mod axum;

pub use config::{AuthConfig, ExecutionLimits, Permission, RemoteSqlConfig};
pub use error::{ErrorResponse, RemoteSqlError, RemoteSqlErrorCode};
pub use protocol::{
    ColumnInfo, ExecuteDone, QueryRows, RemoteSqlAction, RemoteSqlRequest, RemoteSqlResponse,
    SqlBlob, SqlBlobType, SqlValue,
};
pub use service::{RemoteSqlService, RequestContext};
pub use sql_kind::{classify_sql, StatementKind};

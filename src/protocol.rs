use serde::{Deserialize, Serialize};

/// Remote SQL operation requested by the client.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteSqlAction {
    /// Execute a statement and return rows.
    Query,
    /// Execute a statement and return write metadata.
    Execute,
}

/// JSON request accepted by [`crate::RemoteSqlService`].
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RemoteSqlRequest {
    /// Requested operation.
    pub action: RemoteSqlAction,
    /// A single SQLite statement.
    pub sql: String,
    /// Positional parameters bound to `?` placeholders.
    #[serde(default)]
    pub params: Vec<SqlValue>,
}

/// SQLite value represented in the remote JSON protocol.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(untagged)]
pub enum SqlValue {
    /// SQL NULL.
    Null,
    /// Boolean value.
    Bool(bool),
    /// Signed integer value.
    Integer(i64),
    /// Floating point value.
    Real(f64),
    /// UTF-8 text value.
    Text(String),
    /// Binary value encoded as base64.
    Blob(SqlBlob),
}

/// JSON representation for a SQLite BLOB value.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SqlBlob {
    /// Value discriminator. Must be `"blob"`.
    #[serde(rename = "type")]
    pub value_type: SqlBlobType,
    /// Base64-encoded bytes.
    pub base64: String,
}

impl SqlBlob {
    /// Creates a blob value from base64 text.
    #[must_use]
    pub fn new(base64: impl Into<String>) -> Self {
        Self {
            value_type: SqlBlobType::Blob,
            base64: base64.into(),
        }
    }
}

/// Blob discriminator used by [`SqlBlob`].
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SqlBlobType {
    /// A SQLite BLOB.
    Blob,
}

/// Metadata for one returned column.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ColumnInfo {
    /// Column name.
    pub name: String,
    /// SQLite type information exposed by SQLx.
    #[serde(rename = "type")]
    pub type_name: String,
}

/// Rows returned by a query operation.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct QueryRows {
    /// Returned columns.
    pub columns: Vec<ColumnInfo>,
    /// Returned row values.
    pub rows: Vec<Vec<SqlValue>>,
}

/// Write metadata returned by an execute operation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ExecuteDone {
    /// Number of affected rows reported by SQLite.
    pub rows_affected: u64,
    /// Last inserted row id reported by SQLite for this connection.
    pub last_insert_rowid: i64,
}

/// JSON response returned by [`crate::RemoteSqlService`].
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RemoteSqlResponse {
    /// Rows returned by a query operation.
    Rows(QueryRows),
    /// Write metadata returned by an execute operation.
    Done(ExecuteDone),
}

use std::net::IpAddr;
use std::sync::Arc;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use futures_util::TryStreamExt;
use sqlx::sqlite::SqliteRow;
use sqlx::{AssertSqlSafe, Column, Row, SqlitePool, TypeInfo, ValueRef};
use tokio::time::timeout;

use crate::auth::validate_auth;
use crate::{
    classify_sql, ColumnInfo, ExecuteDone, QueryRows, RemoteSqlConfig, RemoteSqlError,
    RemoteSqlRequest, RemoteSqlResponse, SqlBlob, SqlValue,
};

/// Request metadata supplied by framework bindings.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RequestContext {
    /// Bearer token extracted from the request.
    pub bearer_token: Option<String>,
    /// Peer IP address from the accepted connection.
    pub peer_ip: Option<IpAddr>,
}

impl RequestContext {
    /// Creates an empty request context.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the bearer token.
    #[must_use]
    pub fn with_bearer_token(mut self, bearer_token: impl Into<String>) -> Self {
        self.bearer_token = Some(bearer_token.into());
        self
    }

    /// Sets the peer IP address.
    #[must_use]
    pub fn with_peer_ip(mut self, peer_ip: IpAddr) -> Self {
        self.peer_ip = Some(peer_ip);
        self
    }
}

/// Core remote SQL service.
#[derive(Clone, Debug)]
pub struct RemoteSqlService {
    pool: SqlitePool,
    config: Arc<RemoteSqlConfig>,
}

impl RemoteSqlService {
    /// Creates a remote SQL service over a local SQLite pool.
    #[must_use]
    pub fn new(pool: SqlitePool, config: RemoteSqlConfig) -> Self {
        Self {
            pool,
            config: Arc::new(config),
        }
    }

    /// Handles one remote SQL request.
    ///
    /// # Errors
    ///
    /// Returns [`RemoteSqlError`] when authentication fails, limits are
    /// exceeded, SQL is invalid or disallowed, or SQLite execution fails.
    pub async fn handle(
        &self,
        request: RemoteSqlRequest,
        context: RequestContext,
    ) -> Result<RemoteSqlResponse, RemoteSqlError> {
        validate_auth(
            &self.config.auth,
            context.bearer_token.as_deref(),
            context.peer_ip,
        )?;
        self.validate_request(&request)?;

        let statement = classify_sql(&request.sql)?;
        if !self.config.permission.allows(statement) {
            return Err(RemoteSqlError::PermissionDenied {
                permission: self.config.permission,
                statement,
            });
        }

        timeout(self.config.limits.timeout, self.execute(request))
            .await
            .map_err(|_| RemoteSqlError::Timeout)?
    }

    fn validate_request(&self, request: &RemoteSqlRequest) -> Result<(), RemoteSqlError> {
        if request.sql.len() > self.config.limits.max_sql_bytes {
            return Err(RemoteSqlError::SqlTooLarge {
                max: self.config.limits.max_sql_bytes,
            });
        }

        if request.params.len() > self.config.limits.max_params {
            return Err(RemoteSqlError::TooManyParams {
                max: self.config.limits.max_params,
            });
        }

        Ok(())
    }

    async fn execute(
        &self,
        request: RemoteSqlRequest,
    ) -> Result<RemoteSqlResponse, RemoteSqlError> {
        match request.action {
            crate::RemoteSqlAction::Query => self.query(request).await,
            crate::RemoteSqlAction::Execute => self.execute_statement(request).await,
        }
    }

    async fn query(&self, request: RemoteSqlRequest) -> Result<RemoteSqlResponse, RemoteSqlError> {
        let mut query = sqlx::query(AssertSqlSafe(request.sql.as_str()));
        for param in &request.params {
            query = bind_sql_value(query, param)?;
        }

        let mut stream = query.fetch(&self.pool);
        let mut columns = None;
        let mut rows = Vec::new();

        while let Some(row) = stream.try_next().await? {
            if rows.len() >= self.config.limits.max_rows {
                return Err(RemoteSqlError::TooManyRows {
                    max: self.config.limits.max_rows,
                });
            }

            columns.get_or_insert_with(|| column_info(&row));
            rows.push(row_values(&row)?);
        }

        Ok(RemoteSqlResponse::Rows(QueryRows {
            columns: columns.unwrap_or_default(),
            rows,
        }))
    }

    async fn execute_statement(
        &self,
        request: RemoteSqlRequest,
    ) -> Result<RemoteSqlResponse, RemoteSqlError> {
        let mut query = sqlx::query(AssertSqlSafe(request.sql.as_str()));
        for param in &request.params {
            query = bind_sql_value(query, param)?;
        }

        let result = query.execute(&self.pool).await?;
        Ok(RemoteSqlResponse::Done(ExecuteDone {
            rows_affected: result.rows_affected(),
            last_insert_rowid: result.last_insert_rowid(),
        }))
    }
}

fn bind_sql_value<'q>(
    query: sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments>,
    value: &'q SqlValue,
) -> Result<sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments>, RemoteSqlError> {
    match value {
        SqlValue::Null => Ok(query.bind(Option::<i64>::None)),
        SqlValue::Bool(value) => Ok(query.bind(*value)),
        SqlValue::Integer(value) => Ok(query.bind(*value)),
        SqlValue::Real(value) => Ok(query.bind(*value)),
        SqlValue::Text(value) => Ok(query.bind(value)),
        SqlValue::Blob(blob) => Ok(query.bind(decode_blob(blob)?)),
    }
}

fn decode_blob(blob: &SqlBlob) -> Result<Vec<u8>, RemoteSqlError> {
    STANDARD
        .decode(&blob.base64)
        .map_err(|source| RemoteSqlError::InvalidBlobBase64 { source })
}

fn column_info(row: &SqliteRow) -> Vec<ColumnInfo> {
    row.columns()
        .iter()
        .map(|column| ColumnInfo {
            name: column.name().to_owned(),
            type_name: column.type_info().name().to_owned(),
        })
        .collect()
}

fn row_values(row: &SqliteRow) -> Result<Vec<SqlValue>, sqlx::Error> {
    (0..row.len()).map(|index| row_value(row, index)).collect()
}

fn row_value(row: &SqliteRow, index: usize) -> Result<SqlValue, sqlx::Error> {
    let value_type = {
        let raw = row.try_get_raw(index)?;
        if raw.is_null() {
            return Ok(SqlValue::Null);
        }
        raw.type_info().name().to_ascii_uppercase()
    };

    match value_type.as_str() {
        "INTEGER" | "INT" => row.try_get::<i64, _>(index).map(SqlValue::Integer),
        "REAL" | "FLOAT" | "DOUBLE" => row.try_get::<f64, _>(index).map(SqlValue::Real),
        "TEXT" | "CHAR" | "VARCHAR" => row.try_get::<String, _>(index).map(SqlValue::Text),
        "BLOB" => row
            .try_get::<Vec<u8>, _>(index)
            .map(|bytes| SqlValue::Blob(SqlBlob::new(STANDARD.encode(bytes)))),
        _ => row.try_get::<String, _>(index).map(SqlValue::Text),
    }
}

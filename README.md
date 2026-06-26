# sqlx_remote_run

`sqlx_remote_run` lets a service expose a small, guarded JSON API for running SQL against a local `sqlx::SqlitePool`.

It is intended for internal tools, local admin panels, debugging endpoints, and controlled maintenance paths where the database is local SQLite but the SQL request needs to come from another process.

## Features

- SQLite only, backed by `sqlx::SqlitePool`.
- Single-statement SQL execution.
- Positional SQLite parameters using `?`.
- Static bearer token authentication.
- Optional peer IP allowlist.
- Permission tiers: `ReadOnly`, `ReadWrite`, `Admin`.
- Query and execute response types.
- Optional Axum binding behind the `axum` feature.

## Install

Use it from crates.io:

```toml
[dependencies]
sqlx_remote_run = { version = "0.1", features = ["axum"] }

axum = "0.8"
serde_json = "1"
sqlx = { version = "0.9", default-features = false, features = ["sqlite", "runtime-tokio"] }
tokio = { version = "1", features = ["macros", "rt-multi-thread", "net"] }
```

If you only need the framework-neutral core API, omit the feature:

```toml
sqlx_remote_run = "0.1"
```

## Core Usage

Use `RemoteSqlService` directly when you already have your own transport or web framework.

```rust
use sqlx::sqlite::SqlitePoolOptions;
use sqlx_remote_run::{
    AuthConfig, Permission, RemoteSqlAction, RemoteSqlConfig, RemoteSqlRequest,
    RemoteSqlService, RequestContext, SqlValue,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await?;

    sqlx::query("create table users(id integer primary key, name text not null)")
        .execute(&pool)
        .await?;
    sqlx::query("insert into users(id, name) values (1, 'Ada')")
        .execute(&pool)
        .await?;

    let service = RemoteSqlService::new(
        pool,
        RemoteSqlConfig::new(AuthConfig::bearer_token("secret"), Permission::ReadOnly),
    );

    let request = RemoteSqlRequest {
        action: RemoteSqlAction::Query,
        sql: "select id, name from users where id = ?".to_owned(),
        params: vec![SqlValue::Integer(1)],
    };
    let context = RequestContext::new().with_bearer_token("secret");

    let response = service.handle(request, context).await?;
    println!("{}", serde_json::to_string_pretty(&response)?);

    Ok(())
}
```

## Axum Usage

Enable the `axum` feature and mount the router wherever you want the remote SQL endpoint to live.

```rust
use std::net::SocketAddr;

use sqlx::sqlite::SqlitePoolOptions;
use sqlx_remote_run::{AuthConfig, Permission, RemoteSqlConfig, RemoteSqlService};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect("sqlite://app.db")
        .await?;

    let service = RemoteSqlService::new(
        pool,
        RemoteSqlConfig::new(AuthConfig::bearer_token("change-me"), Permission::ReadOnly),
    );

    let app = axum::Router::new().nest("/remote-sql", sqlx_remote_run::axum::router(service));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000").await?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;

    Ok(())
}
```

`into_make_service_with_connect_info::<SocketAddr>()` is required if you configure `allowed_ips`, because the library reads the real peer IP from Axum's connection info.

## HTTP API

The Axum binding handles:

```text
POST /remote-sql/
Authorization: Bearer change-me
Content-Type: application/json
```

Query example:

```bash
curl -s http://127.0.0.1:3000/remote-sql/ \
  -H 'Authorization: Bearer change-me' \
  -H 'Content-Type: application/json' \
  -d '{
    "action": "query",
    "sql": "select id, name from users where id = ?",
    "params": [1]
  }'
```

Query response:

```json
{
  "type": "rows",
  "columns": [
    { "name": "id", "type": "INTEGER" },
    { "name": "name", "type": "TEXT" }
  ],
  "rows": [[1, "Ada"]]
}
```

Execute example:

```bash
curl -s http://127.0.0.1:3000/remote-sql/ \
  -H 'Authorization: Bearer change-me' \
  -H 'Content-Type: application/json' \
  -d '{
    "action": "execute",
    "sql": "insert into users(name) values (?)",
    "params": ["Grace"]
  }'
```

Execute response:

```json
{
  "type": "done",
  "rows_affected": 1,
  "last_insert_rowid": 2
}
```

## Parameters

`params` are positional and bind to SQLite `?` placeholders.

Supported JSON values:

- `null`
- boolean
- integer
- float
- string
- blob as `{ "type": "blob", "base64": "AQID" }`

Named parameters are not supported in the first version.

## Permissions

`Permission::ReadOnly` allows read statements:

- `SELECT`
- read query forms parsed as SQL queries

`Permission::ReadWrite` allows read statements plus DML:

- `INSERT`
- `UPDATE`
- `DELETE`
- `REPLACE`

`Permission::Admin` allows read/write plus administrative statements:

- `CREATE`
- `ALTER`
- `DROP`
- `PRAGMA`
- `VACUUM`
- `ANALYZE`
- `REINDEX`

Unsupported or unknown statements are rejected by default.

## Limits

Default limits:

- `max_sql_bytes`: 64 KiB
- `max_params`: 256
- `max_rows`: 1000
- `timeout`: 30 seconds

Override them with `RemoteSqlConfig::with_limits`:

```rust
use std::time::Duration;

use sqlx_remote_run::{AuthConfig, ExecutionLimits, Permission, RemoteSqlConfig};

let config = RemoteSqlConfig::new(AuthConfig::bearer_token("change-me"), Permission::ReadOnly)
    .with_limits(ExecutionLimits {
        max_sql_bytes: 16 * 1024,
        max_params: 64,
        max_rows: 100,
        timeout: Duration::from_secs(5),
    });
```

## IP Allowlist

Use `AuthConfig::with_allowed_ip` or `with_allowed_ips` to restrict callers by peer IP:

```rust
use sqlx_remote_run::AuthConfig;

let allowed_ip = "127.0.0.1/32".parse().expect("valid CIDR");
let auth = AuthConfig::bearer_token("change-me").with_allowed_ip(allowed_ip);
```

When an allowlist is configured, requests without a peer IP are rejected. The Axum binding uses the real socket peer IP and does not trust `X-Forwarded-For` or `Forwarded` headers.

## Errors

Framework bindings return JSON errors:

```json
{
  "code": "permission_denied",
  "message": "permission ReadOnly does not allow Write statements"
}
```

Common HTTP status mapping:

- `401`: missing or invalid bearer token
- `403`: IP rejected or permission denied
- `400`: invalid JSON, invalid SQL shape, unsupported statement, or limit failure
- `408`: execution timeout
- `422`: SQLite execution error

## Security Notes

This crate intentionally executes remote SQL. Keep the endpoint private, require a strong token, prefer an IP allowlist, and expose only the minimum permission tier needed.

The library validates that the request is a single statement and rejects unknown statement kinds, but it does not make arbitrary SQL safe for public internet exposure.

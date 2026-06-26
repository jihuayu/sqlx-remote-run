use std::time::Duration;

use sqlx::sqlite::SqlitePoolOptions;
use sqlx::SqlitePool;
use sqlx_remote_run::{
    AuthConfig, ExecutionLimits, Permission, RemoteSqlAction, RemoteSqlConfig, RemoteSqlError,
    RemoteSqlRequest, RemoteSqlResponse, RemoteSqlService, RequestContext, SqlBlob, SqlValue,
};

async fn pool() -> SqlitePool {
    SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap()
}

async fn seeded_pool() -> SqlitePool {
    let pool = pool().await;
    sqlx::query("create table users(id integer primary key, name text not null)")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("insert into users(id, name) values (1, 'Ada'), (2, 'Linus')")
        .execute(&pool)
        .await
        .unwrap();
    pool
}

fn service(pool: SqlitePool, permission: Permission) -> RemoteSqlService {
    RemoteSqlService::new(
        pool,
        RemoteSqlConfig::new(AuthConfig::bearer_token("secret"), permission),
    )
}

fn context() -> RequestContext {
    RequestContext::new().with_bearer_token("secret")
}

#[tokio::test]
async fn readonly_query_returns_rows_for_select() {
    let service = service(seeded_pool().await, Permission::ReadOnly);
    let request = RemoteSqlRequest {
        action: RemoteSqlAction::Query,
        sql: "select id, name from users where id = ?".to_owned(),
        params: vec![SqlValue::Integer(1)],
    };

    let response = service.handle(request, context()).await.unwrap();

    let RemoteSqlResponse::Rows(rows) = response else {
        panic!("expected rows response");
    };
    assert_eq!(
        rows.rows,
        vec![vec![SqlValue::Integer(1), SqlValue::Text("Ada".to_owned())]]
    );
}

#[tokio::test]
async fn readonly_execute_rejects_insert() {
    let service = service(seeded_pool().await, Permission::ReadOnly);
    let request = RemoteSqlRequest {
        action: RemoteSqlAction::Execute,
        sql: "insert into users(id, name) values (?, ?)".to_owned(),
        params: vec![SqlValue::Integer(3), SqlValue::Text("Grace".to_owned())],
    };

    let error = service.handle(request, context()).await.unwrap_err();

    assert!(matches!(error, RemoteSqlError::PermissionDenied { .. }));
}

#[tokio::test]
async fn readwrite_execute_allows_insert() {
    let pool = seeded_pool().await;
    let service = service(pool.clone(), Permission::ReadWrite);
    let request = RemoteSqlRequest {
        action: RemoteSqlAction::Execute,
        sql: "insert into users(id, name) values (?, ?)".to_owned(),
        params: vec![SqlValue::Integer(3), SqlValue::Text("Grace".to_owned())],
    };

    let response = service.handle(request, context()).await.unwrap();

    let RemoteSqlResponse::Done(done) = response else {
        panic!("expected done response");
    };
    assert_eq!(done.rows_affected, 1);
}

#[tokio::test]
async fn readwrite_execute_rejects_ddl() {
    let service = service(seeded_pool().await, Permission::ReadWrite);
    let request = RemoteSqlRequest {
        action: RemoteSqlAction::Execute,
        sql: "create table audit(id integer primary key)".to_owned(),
        params: Vec::new(),
    };

    let error = service.handle(request, context()).await.unwrap_err();

    assert!(matches!(error, RemoteSqlError::PermissionDenied { .. }));
}

#[tokio::test]
async fn admin_execute_allows_create_table() {
    let pool = pool().await;
    let service = service(pool.clone(), Permission::Admin);
    let request = RemoteSqlRequest {
        action: RemoteSqlAction::Execute,
        sql: "create table audit(id integer primary key)".to_owned(),
        params: Vec::new(),
    };

    let response = service.handle(request, context()).await.unwrap();

    let RemoteSqlResponse::Done(done) = response else {
        panic!("expected done response");
    };
    assert_eq!(done.rows_affected, 0);
}

#[tokio::test]
async fn handle_rejects_too_many_params() {
    let mut config = RemoteSqlConfig::new(AuthConfig::bearer_token("secret"), Permission::ReadOnly);
    config.limits.max_params = 0;
    let service = RemoteSqlService::new(seeded_pool().await, config);
    let request = RemoteSqlRequest {
        action: RemoteSqlAction::Query,
        sql: "select ?".to_owned(),
        params: vec![SqlValue::Integer(1)],
    };

    let error = service.handle(request, context()).await.unwrap_err();

    assert!(matches!(error, RemoteSqlError::TooManyParams { max: 0 }));
}

#[tokio::test]
async fn handle_rejects_too_many_rows() {
    let mut config = RemoteSqlConfig::new(AuthConfig::bearer_token("secret"), Permission::ReadOnly);
    config.limits.max_rows = 1;
    let service = RemoteSqlService::new(seeded_pool().await, config);
    let request = RemoteSqlRequest {
        action: RemoteSqlAction::Query,
        sql: "select id from users order by id".to_owned(),
        params: Vec::new(),
    };

    let error = service.handle(request, context()).await.unwrap_err();

    assert!(matches!(error, RemoteSqlError::TooManyRows { max: 1 }));
}

#[tokio::test]
async fn query_round_trips_blob_values_as_base64() {
    let pool = pool().await;
    sqlx::query("create table files(id integer primary key, data blob)")
        .execute(&pool)
        .await
        .unwrap();
    let service = service(pool, Permission::ReadWrite);
    let insert = RemoteSqlRequest {
        action: RemoteSqlAction::Execute,
        sql: "insert into files(id, data) values (?, ?)".to_owned(),
        params: vec![SqlValue::Integer(1), SqlValue::Blob(SqlBlob::new("AQID"))],
    };

    service.handle(insert, context()).await.unwrap();
    let query = RemoteSqlRequest {
        action: RemoteSqlAction::Query,
        sql: "select data from files where id = ?".to_owned(),
        params: vec![SqlValue::Integer(1)],
    };
    let response = service.handle(query, context()).await.unwrap();

    let RemoteSqlResponse::Rows(rows) = response else {
        panic!("expected rows response");
    };
    assert_eq!(rows.rows, vec![vec![SqlValue::Blob(SqlBlob::new("AQID"))]]);
}

#[tokio::test]
async fn execute_rejects_invalid_blob_base64() {
    let service = service(seeded_pool().await, Permission::ReadWrite);
    let request = RemoteSqlRequest {
        action: RemoteSqlAction::Execute,
        sql: "insert into users(id, name) values (?, ?)".to_owned(),
        params: vec![
            SqlValue::Integer(3),
            SqlValue::Blob(SqlBlob::new("not valid base64")),
        ],
    };

    let error = service.handle(request, context()).await.unwrap_err();

    assert!(matches!(error, RemoteSqlError::InvalidBlobBase64 { .. }));
}

#[tokio::test]
async fn handle_rejects_sql_that_exceeds_byte_limit() {
    let limits = ExecutionLimits {
        max_sql_bytes: 3,
        max_params: 256,
        max_rows: 1_000,
        timeout: Duration::from_secs(30),
    };
    let config = RemoteSqlConfig::new(AuthConfig::bearer_token("secret"), Permission::ReadOnly)
        .with_limits(limits);
    let service = RemoteSqlService::new(seeded_pool().await, config);
    let request = RemoteSqlRequest {
        action: RemoteSqlAction::Query,
        sql: "select 1".to_owned(),
        params: Vec::new(),
    };

    let error = service.handle(request, context()).await.unwrap_err();

    assert!(matches!(error, RemoteSqlError::SqlTooLarge { max: 3 }));
}

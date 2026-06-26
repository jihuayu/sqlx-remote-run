#![cfg(feature = "axum")]

use axum::body::{to_bytes, Body};
use axum::http::{header, Method, Request, StatusCode};
use serde_json::{json, Value};
use sqlx::sqlite::SqlitePoolOptions;
use sqlx_remote_run::{
    axum as remote_axum, AuthConfig, Permission, RemoteSqlConfig, RemoteSqlService,
};
use tower::ServiceExt;

async fn app() -> axum::Router {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::query("create table users(id integer primary key, name text not null)")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("insert into users(id, name) values (1, 'Ada')")
        .execute(&pool)
        .await
        .unwrap();

    let service = RemoteSqlService::new(
        pool,
        RemoteSqlConfig::new(AuthConfig::bearer_token("secret"), Permission::ReadOnly),
    );
    remote_axum::router(service)
}

async fn response_body(response: axum::response::Response) -> Value {
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn axum_router_returns_query_rows() {
    let app = app().await;
    let request = Request::builder()
        .method(Method::POST)
        .uri("/")
        .header(header::AUTHORIZATION, "Bearer secret")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({
                "action": "query",
                "sql": "select id, name from users where id = ?",
                "params": [1]
            })
            .to_string(),
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    let status = response.status();
    let body = response_body(response).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["type"], "rows");
}

#[tokio::test]
async fn axum_router_returns_unauthorized_for_missing_token() {
    let app = app().await;
    let request = Request::builder()
        .method(Method::POST)
        .uri("/")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({
                "action": "query",
                "sql": "select 1",
                "params": []
            })
            .to_string(),
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    let status = response.status();
    let body = response_body(response).await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["code"], "unauthorized");
}

#[tokio::test]
async fn axum_router_returns_json_error_for_invalid_json() {
    let app = app().await;
    let request = Request::builder()
        .method(Method::POST)
        .uri("/")
        .header(header::AUTHORIZATION, "Bearer secret")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from("{"))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    let status = response.status();
    let body = response_body(response).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "invalid_request");
}

//! Axum binding for the remote SQL service.

use std::net::SocketAddr;

use ::axum::body::to_bytes;
use ::axum::extract::{ConnectInfo, State};
use ::axum::http::{header, HeaderMap, Request, StatusCode};
use ::axum::response::{IntoResponse, Response};
use ::axum::routing::post;
use ::axum::{Json, Router};

use crate::{
    ErrorResponse, RemoteSqlError, RemoteSqlErrorCode, RemoteSqlRequest, RemoteSqlResponse,
    RemoteSqlService, RequestContext,
};

/// Builds an Axum router that handles `POST /`.
///
/// Callers usually mount this under their chosen path:
///
/// ```no_run
/// # use sqlx_remote_run::{RemoteSqlConfig, RemoteSqlService};
/// # async fn build(pool: sqlx::SqlitePool) {
/// let service = RemoteSqlService::new(pool, RemoteSqlConfig::default());
/// let app = axum::Router::new().nest("/remote-sql", sqlx_remote_run::axum::router(service));
/// # let _: axum::Router = app;
/// # }
/// ```
pub fn router(service: RemoteSqlService) -> Router {
    Router::new().route("/", post(handle)).with_state(service)
}

async fn handle(
    State(service): State<RemoteSqlService>,
    request: Request<::axum::body::Body>,
) -> Result<Json<RemoteSqlResponse>, HttpError> {
    let (parts, body) = request.into_parts();
    let payload = to_bytes(body, usize::MAX)
        .await
        .map_err(|err| HttpError::bad_request(err.to_string()))?;
    let request = serde_json::from_slice::<RemoteSqlRequest>(&payload)
        .map_err(|err| HttpError::bad_request(err.to_string()))?;
    let context = RequestContext {
        bearer_token: bearer_token(&parts.headers),
        peer_ip: parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|connect_info| connect_info.0.ip()),
    };

    service
        .handle(request, context)
        .await
        .map(Json)
        .map_err(HttpError::Core)
}

fn bearer_token(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    if scheme.eq_ignore_ascii_case("bearer") && !token.is_empty() {
        Some(token.to_owned())
    } else {
        None
    }
}

enum HttpError {
    Core(RemoteSqlError),
    BadRequest(String),
}

impl HttpError {
    fn bad_request(message: String) -> Self {
        Self::BadRequest(message)
    }
}

impl IntoResponse for HttpError {
    fn into_response(self) -> Response {
        match self {
            Self::Core(error) => {
                let status = status_for_error(&error);
                (status, Json(ErrorResponse::from_error(&error))).into_response()
            }
            Self::BadRequest(message) => (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    code: RemoteSqlErrorCode::InvalidRequest,
                    message,
                }),
            )
                .into_response(),
        }
    }
}

fn status_for_error(error: &RemoteSqlError) -> StatusCode {
    match error {
        RemoteSqlError::Unauthorized => StatusCode::UNAUTHORIZED,
        RemoteSqlError::ForbiddenIp
        | RemoteSqlError::MissingPeerIp
        | RemoteSqlError::PermissionDenied { .. } => StatusCode::FORBIDDEN,
        RemoteSqlError::SqlExecution(_) => StatusCode::UNPROCESSABLE_ENTITY,
        RemoteSqlError::Timeout => StatusCode::REQUEST_TIMEOUT,
        RemoteSqlError::EmptySql
        | RemoteSqlError::InvalidSql(_)
        | RemoteSqlError::ExpectedSingleStatement
        | RemoteSqlError::UnsupportedStatement
        | RemoteSqlError::SqlTooLarge { .. }
        | RemoteSqlError::TooManyParams { .. }
        | RemoteSqlError::TooManyRows { .. }
        | RemoteSqlError::InvalidBlobBase64 { .. } => StatusCode::BAD_REQUEST,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bearer_token_extracts_case_insensitive_bearer_scheme() {
        let mut headers = HeaderMap::new();
        headers.insert(header::AUTHORIZATION, "bearer secret".parse().unwrap());

        let token = bearer_token(&headers);

        assert_eq!(token.as_deref(), Some("secret"));
    }
}

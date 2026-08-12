mod auth;
mod config;
mod ctdp;
#[cfg(all(test, feature = "db-tests"))]
mod db_tests;
mod domain;
mod error;
mod models;
mod notifications;
mod rsip;

use std::{sync::Arc, time::Duration};

use axum::{
    Router,
    http::{HeaderName, StatusCode},
    routing::{any, get, post},
};
use config::Config;
use error::{ApiError, ApiResult};
use sqlx::{PgPool, postgres::PgPoolOptions};
use tokio::signal;
use tower_http::{
    catch_panic::CatchPanicLayer,
    compression::CompressionLayer,
    limit::RequestBodyLimitLayer,
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    sensitive_headers::SetSensitiveRequestHeadersLayer,
    services::{ServeDir, ServeFile},
    timeout::TimeoutLayer,
    trace::TraceLayer,
};
use tracing::{info, warn};

#[derive(Clone)]
pub struct AppState {
    pool: PgPool,
    config: Arc<Config>,
    oidc: auth::OidcRuntime,
    notifications: notifications::NotificationRuntime,
}

#[tokio::main]
async fn main() -> ApiResult<()> {
    init_tracing();
    let config = Arc::new(Config::from_env()?);
    let pool = PgPoolOptions::new()
        .max_connections(20)
        .min_connections(2)
        .acquire_timeout(Duration::from_secs(5))
        .connect(&config.database_url)
        .await?;
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .map_err(|error| ApiError::Internal(format!("数据库初始化失败: {error}")))?;
    let oidc = auth::OidcRuntime::discover(&config).await?;
    let notifications = notifications::NotificationRuntime::from_config(&config)?;
    let state = AppState {
        pool: pool.clone(),
        config: config.clone(),
        oidc,
        notifications,
    };
    let app = router(state.clone());
    let listener = tokio::net::TcpListener::bind(config.bind)
        .await
        .map_err(|error| ApiError::Internal(format!("监听地址失败: {error}")))?;
    info!(address = %config.bind, "Lumeno API listening");

    spawn_maintenance(state);
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(|error| ApiError::Internal(format!("HTTP 服务失败: {error}")))?;
    Ok(())
}

fn router(state: AppState) -> Router {
    let request_id_header = HeaderName::from_static("x-request-id");
    let static_dir = state.config.static_dir.clone();
    let index = static_dir.join("index.html");
    Router::new()
        .route("/health/live", get(auth::health))
        .route("/health/ready", get(auth::ready))
        .route("/api/v1/auth/login", get(auth::login))
        .route("/api/v1/auth/callback", get(auth::callback))
        .route(
            "/api/v1/auth/backchannel-logout",
            post(auth::backchannel_logout),
        )
        .route("/api/v1/auth/logout", post(auth::logout))
        .route("/api/v1/me", get(auth::me).put(auth::update_settings))
        .merge(ctdp::router())
        .merge(rsip::router())
        .merge(notifications::router())
        .route("/api/{*path}", any(api_not_found))
        .fallback_service(ServeDir::new(static_dir).fallback(ServeFile::new(index)))
        .with_state(state)
        .layer(PropagateRequestIdLayer::new(request_id_header.clone()))
        .layer(SetRequestIdLayer::new(request_id_header, MakeRequestUuid))
        .layer(SetSensitiveRequestHeadersLayer::new(std::iter::once(
            axum::http::header::COOKIE,
        )))
        .layer(RequestBodyLimitLayer::new(256 * 1024))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(15),
        ))
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
        .layer(CatchPanicLayer::new())
}

async fn api_not_found() -> ApiError {
    ApiError::NotFound
}

fn spawn_maintenance(state: AppState) {
    let notification_state = state.clone();
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(15));
        loop {
            ticker.tick().await;
            if let Err(error) = ctdp::reconcile_due_states(&state.pool).await {
                warn!(?error, "failed to reconcile protocol states");
            }
            if let Err(error) = auth::cleanup_auth(&state.pool).await {
                warn!(?error, "failed to clean auth state");
            }
        }
    });
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(15));
        loop {
            ticker.tick().await;
            if let Err(error) = notifications::process_jobs(&notification_state).await {
                warn!(?error, "failed to process notification jobs");
            }
            if let Err(error) = notifications::cleanup_jobs(&notification_state.pool).await {
                warn!(?error, "failed to clean notification jobs");
            }
        }
    });
}

async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(error) = signal::ctrl_c().await {
            warn!(?error, "failed to receive Ctrl+C signal");
            std::future::pending::<()>().await;
        }
    };
    #[cfg(unix)]
    let terminate = async {
        match signal::unix::signal(signal::unix::SignalKind::terminate()) {
            Ok(mut stream) => {
                stream.recv().await;
            }
            Err(error) => {
                warn!(?error, "failed to install termination signal handler");
                std::future::pending::<()>().await;
            }
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! { _ = ctrl_c => {}, _ = terminate => {} }
}

fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "lumeno_api=info,tower_http=info".into());
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .json()
        .init();
}

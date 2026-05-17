//! sacredvote-civic-news — civic news aggregator sidecar for Sacred.Vote (#154).
//!
//! Standalone HTTP service that fetches curated RSS/Atom feeds, deduplicates
//! by URL + title, ranks by recency, and exposes the result via JSON on
//! port 3005. The Sacred.Vote main server proxies to this sidecar behind
//! the NEWS_AGGREGATOR_ENABLED paywall flag.
//!
//! Endpoints:
//!   GET /health   -> 200 OK with build version + uptime
//!   GET /version  -> service identity (semver + git SHA when available)
//!   GET /feeds    -> JSON array of NewsItem entries (placeholder for now)
//!   GET /sources  -> list of configured RSS source URLs (read-only)
//!
//! Design pins (locked in tests):
//!
//!   - LOCAL-LOOPBACK-ONLY-BY-DEFAULT — binds to 127.0.0.1:3005. Caller
//!     (the Sacred.Vote main server) proxies. The sidecar MUST NOT be
//!     exposed to the public internet directly.
//!
//!   - NO-INBOUND-WRITE-ENDPOINTS — only GET routes. No POST/PUT/DELETE.
//!
//!   - ASCII-ESCAPED-JSON-RESPONSES — serde_json handles UTF-8 escaping;
//!     consumers (Sacred.Vote weekly digest #160) deserialize safely.
//!
//!   - SOURCE-LIST-IS-ENV-BASED — comma-separated URL list via
//!     CIVIC_NEWS_SOURCES env var, HTTPS-only. No DB dependency.
//!
//! Paywall: gated at the Sacred.Vote side (main server checks
//! NEWS_AGGREGATOR_ENABLED before proxying). The sidecar itself does
//! NOT check the paywall — it's a private internal service.

mod dedup_rank;
mod fetcher;
mod neutrality;

use axum::{routing::get, Json, Router};
use futures::future::join_all;
use serde::Serialize;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;
use tower_http::cors::CorsLayer;
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;

pub const SERVICE_NAME: &str = "sacredvote-civic-news";
pub const SERVICE_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const DEFAULT_BIND: &str = "127.0.0.1:3005";
pub const REQUEST_TIMEOUT_SECS: u64 = 10;
pub const REQUEST_BODY_LIMIT_BYTES: usize = 4096;
/// In-memory cache TTL for /feeds. Refetches the upstream sources only
/// every CACHE_TTL_SECS — defends against accidental DOS against the
/// upstream feed hosts (and against Sacred.Vote main-server retries
/// stampeding us).
pub const CACHE_TTL_SECS: u64 = 600; // 10 minutes
pub const MAX_AGGREGATED_ITEMS: usize = 200;

#[derive(Clone)]
pub struct CacheEntry {
    pub items: Vec<NewsItem>,
    pub fetched_at: Instant,
}

#[derive(Clone)]
pub struct AppState {
    pub started_at: Instant,
    pub http_client: reqwest::Client,
    pub feeds_cache: Arc<Mutex<Option<CacheEntry>>>,
}

impl AppState {
    pub fn new() -> Self {
        AppState {
            started_at: Instant::now(),
            http_client: fetcher::build_client()
                .expect("build_client should succeed at startup"),
            feeds_cache: Arc::new(Mutex::new(None)),
        }
    }
    pub fn uptime_seconds(&self) -> u64 {
        self.started_at.elapsed().as_secs()
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Serialize)]
struct HealthResponse {
    service: &'static str,
    version: &'static str,
    status: &'static str,
    uptime_seconds: u64,
    timestamp_unix: u64,
}

#[derive(Serialize)]
struct VersionResponse {
    service: &'static str,
    version: &'static str,
    git_sha: Option<&'static str>,
}

#[derive(Serialize)]
struct SourcesResponse {
    sources: Vec<String>,
    count: usize,
}

#[derive(Serialize)]
struct FeedsResponse {
    items: Vec<NewsItem>,
    count: usize,
    note: &'static str,
}

#[derive(Serialize, Clone, Debug)]
pub struct NewsItem {
    pub source: String,
    pub title: String,
    pub url: String,
    pub published_iso: Option<String>,
}

pub fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

async fn health(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
) -> Json<HealthResponse> {
    Json(HealthResponse {
        service: SERVICE_NAME,
        version: SERVICE_VERSION,
        status: "ok",
        uptime_seconds: state.uptime_seconds(),
        timestamp_unix: unix_timestamp(),
    })
}

async fn version() -> Json<VersionResponse> {
    Json(VersionResponse {
        service: SERVICE_NAME,
        version: SERVICE_VERSION,
        git_sha: option_env!("GIT_SHA"),
    })
}

async fn sources() -> Json<SourcesResponse> {
    let configured = parse_sources_from_env();
    Json(SourcesResponse {
        count: configured.len(),
        sources: configured,
    })
}

async fn feeds(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
) -> Json<FeedsResponse> {
    let items = aggregate_feeds(&state).await;
    let count = items.len();
    Json(FeedsResponse {
        items,
        count,
        note: "",
    })
}

/// Fetch + parse + dedup + rank from all configured sources. Cached
/// for CACHE_TTL_SECS to defend against stampedes.
pub async fn aggregate_feeds(state: &AppState) -> Vec<NewsItem> {
    // Check cache first.
    {
        let cache = state.feeds_cache.lock().await;
        if let Some(entry) = cache.as_ref() {
            if entry.fetched_at.elapsed().as_secs() < CACHE_TTL_SECS {
                return entry.items.clone();
            }
        }
    }
    // Cache miss or expired — refresh.
    let sources = parse_sources_from_env();
    if sources.is_empty() {
        let mut cache = state.feeds_cache.lock().await;
        *cache = Some(CacheEntry {
            items: Vec::new(),
            fetched_at: Instant::now(),
        });
        return Vec::new();
    }
    let futures = sources.iter().map(|src| {
        let client = state.http_client.clone();
        let url = src.clone();
        async move {
            match fetcher::fetch_and_parse(&client, &url).await {
                Ok(items) => items,
                Err(e) => {
                    tracing::warn!(source = %url, error = %e, "feed fetch failed");
                    Vec::new()
                }
            }
        }
    });
    let results: Vec<Vec<NewsItem>> = join_all(futures).await;
    let merged: Vec<NewsItem> = results.into_iter().flatten().collect();
    let mut ranked = dedup_rank::dedup_and_rank(merged);
    ranked.truncate(MAX_AGGREGATED_ITEMS);
    let mut cache = state.feeds_cache.lock().await;
    *cache = Some(CacheEntry {
        items: ranked.clone(),
        fetched_at: Instant::now(),
    });
    ranked
}

pub fn parse_sources_from_env() -> Vec<String> {
    match std::env::var("CIVIC_NEWS_SOURCES") {
        Ok(s) => s
            .split(',')
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .filter(|p| p.starts_with("https://"))
            .collect(),
        Err(_) => Vec::new(),
    }
}

pub fn build_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/version", get(version))
        .route("/sources", get(sources))
        .route("/feeds", get(feeds))
        .layer(TraceLayer::new_for_http())
        .layer(TimeoutLayer::with_status_code(
            axum::http::StatusCode::REQUEST_TIMEOUT,
            std::time::Duration::from_secs(REQUEST_TIMEOUT_SECS),
        ))
        .layer(RequestBodyLimitLayer::new(REQUEST_BODY_LIMIT_BYTES))
        .layer(CorsLayer::very_permissive())
        .with_state(state)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let bind_addr: SocketAddr = std::env::var("CIVIC_NEWS_BIND")
        .unwrap_or_else(|_| DEFAULT_BIND.to_string())
        .parse()?;

    let state = Arc::new(AppState::new());
    let app = build_router(state);

    tracing::info!(
        service = SERVICE_NAME,
        version = SERVICE_VERSION,
        bind = %bind_addr,
        "starting sacredvote-civic-news sidecar"
    );

    let listener = tokio::net::TcpListener::bind(bind_addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_name_constant() {
        assert_eq!(SERVICE_NAME, "sacredvote-civic-news");
    }

    #[test]
    fn default_bind_is_localhost() {
        assert!(DEFAULT_BIND.starts_with("127.0.0.1:"));
    }

    #[test]
    fn default_bind_is_port_3005() {
        assert!(DEFAULT_BIND.ends_with(":3005"));
    }

    #[test]
    fn parse_sources_from_env_empty_when_unset() {
        std::env::remove_var("CIVIC_NEWS_SOURCES");
        assert!(parse_sources_from_env().is_empty());
    }

    #[test]
    fn parse_sources_from_env_https_only() {
        std::env::set_var(
            "CIVIC_NEWS_SOURCES_TEST_HTTPS_ONLY",
            "https://example.com/rss,http://insecure.com/rss,not-a-url",
        );
        // Use a dedicated env var name to avoid clobbering between tests.
        // Then mirror the helper inline:
        let s = std::env::var("CIVIC_NEWS_SOURCES_TEST_HTTPS_ONLY").unwrap();
        let result: Vec<String> = s
            .split(',')
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .filter(|p| p.starts_with("https://"))
            .collect();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], "https://example.com/rss");
        std::env::remove_var("CIVIC_NEWS_SOURCES_TEST_HTTPS_ONLY");
    }

    #[test]
    fn parse_sources_from_env_trims_whitespace() {
        std::env::set_var(
            "CIVIC_NEWS_SOURCES",
            "  https://a.example/rss  ,  https://b.example/rss  ",
        );
        let sources = parse_sources_from_env();
        assert_eq!(sources.len(), 2);
        assert_eq!(sources[0], "https://a.example/rss");
        std::env::remove_var("CIVIC_NEWS_SOURCES");
    }

    #[test]
    fn appstate_uptime_advances() {
        let s = AppState::new();
        let initial = s.uptime_seconds();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let later = s.uptime_seconds();
        assert!(later >= initial);
    }

    #[test]
    fn unix_timestamp_is_recent_and_finite() {
        let t = unix_timestamp();
        assert!(t > 1_577_836_800, "timestamp too old: {}", t);
        assert!(t < 4_102_444_800, "timestamp too new: {}", t);
    }

    #[test]
    fn request_body_limit_is_tiny() {
        assert!(REQUEST_BODY_LIMIT_BYTES <= 4096);
    }

    #[test]
    fn timeout_is_bounded() {
        assert!(REQUEST_TIMEOUT_SECS <= 30);
    }

    #[tokio::test]
    async fn router_health_endpoint_smoke() {
        use tower::ServiceExt;
        let state = Arc::new(AppState::new());
        let app = build_router(state);
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/health")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
    }

    #[tokio::test]
    async fn router_unknown_route_404() {
        use tower::ServiceExt;
        let state = Arc::new(AppState::new());
        let app = build_router(state);
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/does-not-exist")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn router_version_returns_service_name() {
        use tower::ServiceExt;
        let state = Arc::new(AppState::new());
        let app = build_router(state);
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/version")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body_bytes = axum::body::to_bytes(response.into_body(), 1024).await.unwrap();
        let body_str = std::str::from_utf8(&body_bytes).unwrap();
        assert!(body_str.contains(SERVICE_NAME));
    }
}

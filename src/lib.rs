pub mod admin;
pub mod api;
pub mod audit;
pub mod auth;
pub mod config;
pub mod db;
pub mod error;
pub mod models;
pub mod proxy;
pub mod sse;
use anyhow::Result;
use argon2::{Argon2, PasswordHasher, password_hash::SaltString};
use axum::{
    Router,
    extract::DefaultBodyLimit,
    middleware,
    routing::{get, patch, post},
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, RwLock},
    time::{Duration, Instant},
};
use tokio::sync::Semaphore;
use tower_http::cors::{Any, CorsLayer};

#[derive(Clone)]
pub struct App(pub Arc<Inner>);
pub struct Inner {
    pub cfg: config::Config,
    pub db: db::Db,
    pub http: reqwest::Client,
    pub catalog: RwLock<models::Catalog>,
    pub slots: Arc<Semaphore>,
    pub login_slots: Semaphore,
    pub rate: Mutex<HashMap<String, (Instant, u32)>>,
    pub login_rate: Mutex<(Instant, u32)>,
    pub password_hash: String,
}
impl std::ops::Deref for App {
    type Target = Inner;
    fn deref(&self) -> &Inner {
        &self.0
    }
}
impl App {
    pub async fn new(mut cfg: config::Config) -> Result<Self> {
        let catalog = models::Catalog::parse(&std::fs::read_to_string(&cfg.catalog_path)?)?;
        anyhow::ensure!(
            catalog.resolve(&cfg.default_model).is_some(),
            "DEFAULT_MODEL is not in the Go catalog"
        );
        let salt = SaltString::generate(&mut rand::rngs::OsRng);
        let password_hash = Argon2::default()
            .hash_password(cfg.admin_password.as_bytes(), &salt)
            .map_err(|e| anyhow::anyhow!("Password hashing failed: {e}"))?
            .to_string();
        cfg.admin_password.clear();
        let db = db::Db::open(&cfg.database)?;
        db.call(|c| {
            c.execute("DELETE FROM admin_sessions", [])?;
            Ok(())
        })
        .await?;
        let http = reqwest::Client::builder()
            .user_agent(concat!("go-router/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(10))
            .pool_idle_timeout(Duration::from_secs(90))
            .tcp_nodelay(true)
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        let slots = Arc::new(Semaphore::new(cfg.max_concurrent));
        Ok(Self(Arc::new(Inner {
            cfg,
            db,
            http,
            catalog: RwLock::new(catalog),
            slots,
            login_slots: Semaphore::new(2),
            rate: Mutex::new(HashMap::new()),
            login_rate: Mutex::new((Instant::now(), 0)),
            password_hash,
        })))
    }
}
pub fn router(app: App) -> Router {
    let mut cors = CorsLayer::new()
        .allow_methods([
            axum::http::Method::GET,
            axum::http::Method::POST,
            axum::http::Method::OPTIONS,
            axum::http::Method::DELETE,
        ])
        .allow_headers(Any)
        .expose_headers([
            axum::http::HeaderName::from_static("x-generation-id"),
            axum::http::HeaderName::from_static("x-request-id"),
        ]);
    if app.cfg.cors_origins.iter().any(|s| s == "*") {
        cors = cors.allow_origin(Any)
    } else {
        let origins = app
            .cfg
            .cors_origins
            .iter()
            .filter_map(|s| s.parse::<axum::http::HeaderValue>().ok())
            .collect::<Vec<_>>();
        cors = cors.allow_origin(origins);
    }
    let api = Router::new()
        .route("/chat/completions", post(proxy::chat))
        .route("/completions", post(proxy::completions))
        .route("/responses", post(proxy::responses))
        .route("/messages", post(proxy::messages))
        .route("/models", get(api::models))
        .route("/models/user", get(api::user_models))
        .route("/models/count", get(api::model_count))
        .route("/models/{*path}", get(api::model_endpoints))
        .route("/model/{*path}", get(api::model_detail))
        .route("/providers", get(api::providers))
        .route("/key", get(api::key_info))
        .route("/auth/key", get(api::key_info))
        .route("/generation", get(api::generation))
        .route("/generation/content", get(api::generation_content))
        .fallback(api::unsupported)
        .method_not_allowed_fallback(|| async {
            error::ApiError::new(405, "HTTP method not allowed for this API endpoint")
        })
        .layer(cors)
        .layer(middleware::from_fn_with_state(
            app.clone(),
            audit::middleware,
        ));
    let admin = Router::new()
        .route("/session", get(admin::session).delete(admin::logout))
        .route("/login", post(admin::login))
        .route("/stats", get(admin::stats))
        .route("/events", get(admin::events))
        .route("/keys", get(admin::keys).post(admin::create_key))
        .route(
            "/keys/{id}",
            patch(admin::update_key).delete(admin::delete_key),
        )
        .route("/logs", get(admin::logs))
        .route("/logs/{id}", get(admin::log))
        .route("/models", get(admin::models))
        .route("/models/reload", post(admin::reload_models))
        .route("/settings", get(admin::settings));
    Router::new()
        .nest("/api/v1", api.clone())
        .nest("/v1", api)
        .nest("/admin/api", admin)
        .route("/", get(admin::index))
        .route("/admin", get(admin::index))
        .route("/admin/", get(admin::index))
        .route("/admin/app.js", get(admin::js))
        .route("/admin/style.css", get(admin::css))
        .route(
            "/healthz",
            get(|| async {
                axum::Json(serde_json::json!({"status":"ok","version":env!("CARGO_PKG_VERSION")}))
            }),
        )
        .route("/readyz", get(admin::ready))
        .fallback(|| async { error::ApiError::new(404, "Not found") })
        .layer(DefaultBodyLimit::max(app.cfg.max_body_bytes))
        .layer(middleware::from_fn(admin::security_headers))
        .with_state(app)
}

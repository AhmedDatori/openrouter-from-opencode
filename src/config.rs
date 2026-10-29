use anyhow::{Context, Result, bail};
use std::{env, path::PathBuf};

#[derive(Clone)]
pub struct Config {
    pub bind: String,
    pub database: PathBuf,
    pub upstream_base: String,
    pub upstream_key: String,
    pub admin_password: String,
    pub public_url: Option<String>,
    pub cookie_secure: bool,
    pub default_model: String,
    pub catalog_path: PathBuf,
    pub max_concurrent: usize,
    pub max_body_bytes: usize,
    pub max_response_bytes: usize,
    pub max_log_bytes: usize,
    pub log_bodies: bool,
    pub retention_days: i64,
    pub timeout_secs: u64,
    pub idle_timeout_secs: u64,
    pub cors_origins: Vec<String>,
    pub subscription_usd: f64,
}
impl Config {
    pub fn from_env() -> Result<Self> {
        fn val(k: &str, fallback: &str) -> String {
            env::var(k).unwrap_or_else(|_| fallback.into())
        }
        fn num<T: std::str::FromStr>(k: &str, fallback: &str) -> Result<T> {
            val(k, fallback)
                .parse()
                .map_err(|_| anyhow::anyhow!("Invalid {k}"))
        }
        let c = Self {
            bind: val("BIND", "127.0.0.1:8080"),
            database: val("DATABASE_PATH", "data/go-router.db").into(),
            upstream_base: val("GO_BASE_URL", "https://opencode.ai/inference/go")
                .trim_end_matches('/')
                .into(),
            upstream_key: env::var("OPENCODE_GO_API_KEY")
                .context("Set OPENCODE_GO_API_KEY in .env")?,
            admin_password: env::var("ADMIN_PASSWORD").context("Set ADMIN_PASSWORD in .env")?,
            public_url: env::var("PUBLIC_URL")
                .ok()
                .filter(|s| !s.is_empty())
                .map(|s| s.trim_end_matches('/').into()),
            cookie_secure: num("COOKIE_SECURE", "false")?,
            default_model: val("DEFAULT_MODEL", "deepseek/deepseek-v4.1-flash"),
            catalog_path: val("MODEL_CATALOG_PATH", "config/models.json").into(),
            max_concurrent: num("MAX_CONCURRENT_REQUESTS", "8")?,
            max_body_bytes: num("MAX_BODY_BYTES", "20971520")?,
            max_response_bytes: num("MAX_RESPONSE_BYTES", "16777216")?,
            max_log_bytes: num("MAX_LOG_BYTES", "65536")?,
            log_bodies: num("LOG_BODIES", "true")?,
            retention_days: num("LOG_RETENTION_DAYS", "30")?,
            timeout_secs: num("REQUEST_TIMEOUT_SECONDS", "600")?,
            idle_timeout_secs: num("STREAM_IDLE_TIMEOUT_SECONDS", "90")?,
            cors_origins: val("CORS_ORIGINS", "")
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect(),
            subscription_usd: num("MONTHLY_SUBSCRIPTION_USD", "10")?,
        };
        if c.upstream_key.trim().is_empty() || c.upstream_key.starts_with("replace-with-") {
            bail!("OPENCODE_GO_API_KEY cannot be empty");
        }
        if c.admin_password.len() < 16 || c.admin_password.starts_with("replace-with-") {
            bail!("ADMIN_PASSWORD must have at least 16 characters");
        }
        if c.max_concurrent == 0
            || c.max_body_bytes == 0
            || c.max_log_bytes < 1024
            || c.retention_days < 1
            || c.timeout_secs == 0
            || c.idle_timeout_secs == 0
        {
            bail!("Invalid resource limits");
        }
        let url = reqwest::Url::parse(&c.upstream_base).context("Invalid GO_BASE_URL")?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
        {
            bail!("GO_BASE_URL must be an HTTP(S) URL without credentials or query");
        }
        if !c.subscription_usd.is_finite() || c.subscription_usd < 0.0 {
            bail!("Invalid MONTHLY_SUBSCRIPTION_USD");
        }
        Ok(c)
    }
}

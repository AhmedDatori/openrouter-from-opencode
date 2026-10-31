use anyhow::Result;
use go_router::{App, config::Config, router};
use std::time::Duration;
#[tokio::main(worker_threads = 2)]
async fn main() -> Result<()> {
    // A Windows startup task or PowerShell launch may inherit System32 as cwd.
    // Windows packages keep their configuration and catalog beside the executable.
    #[cfg(windows)]
    if let Some(directory) = std::env::current_exe()?.parent() {
        std::env::set_current_dir(directory)?;
    }
    if std::path::Path::new(".env").exists() {
        dotenvy::from_path(".env").map_err(|_| anyhow::anyhow!("Could not load .env. Save it as UTF-8 with valid KEY=value lines. Credential values are not printed."))?;
    }
    if std::env::args().any(|a| a == "--version") {
        println!("go-router {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if std::env::args().any(|a| a == "--help" || a == "-h") {
        println!(
            "go-router — OpenRouter-compatible OpenCode Go gateway\n\nUsage: go-router [--check | --version]\n\nReads .env / environment. See .env.example and README.md."
        );
        return Ok(());
    }
    if std::env::args().any(|a| a == "--healthcheck") {
        let bind = std::env::var("BIND").unwrap_or_else(|_| "127.0.0.1:8080".into());
        let addr: std::net::SocketAddr = bind.parse()?;
        let host = if addr.is_ipv6() { "[::1]" } else { "127.0.0.1" };
        let url = format!("http://{host}:{}/healthz", addr.port());
        reqwest::Client::new()
            .get(url)
            .timeout(Duration::from_secs(3))
            .send()
            .await?
            .error_for_status()?;
        return Ok(());
    }
    let config = Config::from_env()?;
    if std::env::args().any(|a| a == "--check") {
        let cat =
            go_router::models::Catalog::parse(&std::fs::read_to_string(&config.catalog_path)?)?;
        anyhow::ensure!(
            cat.resolve(&config.default_model).is_some(),
            "Unknown DEFAULT_MODEL"
        );
        println!(
            "Configuration valid; {} Go models; bind {}. No upstream call made.",
            cat.models.len(),
            config.bind
        );
        return Ok(());
    }
    let app = App::new(config).await?;
    app.db.cleanup(app.cfg.retention_days).await?;
    let cleanup = app.clone();
    let task = tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(3600)).await;
            if let Err(e) = cleanup.db.cleanup(cleanup.cfg.retention_days).await {
                eprintln!("log cleanup failed: {e}")
            }
        }
    });
    let listener = tokio::net::TcpListener::bind(&app.cfg.bind).await?;
    println!(
        "Go Router {} listening on http://{} — admin /admin — API /api/v1",
        env!("CARGO_PKG_VERSION"),
        listener.local_addr()?
    );
    axum::serve(listener, router(app.clone()))
        .with_graceful_shutdown(shutdown())
        .await?;
    task.abort();
    app.db.flush().await?;
    Ok(())
}
async fn shutdown() {
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("SIGTERM handler");
        tokio::select! {_=tokio::signal::ctrl_c()=>{},_=term.recv()=>{}}
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

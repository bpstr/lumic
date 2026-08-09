#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if std::env::args().nth(1).as_deref() == Some("--version") {
        println!("lumicd {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let facts = lumic_platform::inspect_host()?;
    tracing::info!(
        node = %facts.hostname,
        distribution = %facts.distribution.distribution.id(),
        version = %facts.distribution.version_id,
        architecture = ?facts.architecture,
        cpu_count = facts.cpu_count,
        memory_bytes = facts.memory.total_bytes,
        "lumic daemon started"
    );

    let state_dir = std::env::var_os("LUMIC_STATE_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "/var/lib/lumic".into());
    let apps_root = std::env::var_os("LUMIC_APPS_ROOT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| state_dir.join("apps"));
    let bind: std::net::SocketAddr = std::env::var("LUMIC_UI_BIND")
        .unwrap_or_else(|_| "127.0.0.1:8080".into())
        .parse()?;
    tracing::info!(address = %bind, "operator UI listening");

    tokio::select! {
        result = lumic_ui::serve(lumic_ui::UiState::new(&state_dir, apps_root), bind) => result?,
        result = shutdown_signal() => result?,
    }
    tracing::info!(node = %facts.hostname, "lumic daemon stopped gracefully");
    Ok(())
}

#[cfg(unix)]
async fn shutdown_signal() -> std::io::Result<()> {
    use tokio::signal::unix::{SignalKind, signal};

    let mut terminate = signal(SignalKind::terminate())?;
    tokio::select! {
        result = tokio::signal::ctrl_c() => result,
        _ = terminate.recv() => Ok(()),
    }
}

#[cfg(not(unix))]
async fn shutdown_signal() -> std::io::Result<()> {
    tokio::signal::ctrl_c().await
}

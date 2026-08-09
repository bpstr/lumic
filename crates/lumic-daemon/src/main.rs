#[tokio::main]
async fn main() -> anyhow::Result<()> {
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

    shutdown_signal().await?;
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter("info").init();
    let facts = lumic_platform::inspect_host();
    tracing::info!(os = %facts.os, architecture = %facts.architecture, "lumic daemon started");
    tokio::signal::ctrl_c().await?;
    tracing::info!("lumic daemon stopped");
    Ok(())
}

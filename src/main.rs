#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let settings = kilog::config::load_or_create()?;
    let path = kilog::config::settings_path()?;
    tracing::info!(path = %path.display(), ?settings, "settings loaded");
    Ok(())
}
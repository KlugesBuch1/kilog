use eframe::egui;

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let config = kilog::config::AppConfig::load()?;
    let path = kilog::config::config_path()?;
    tracing::info!(path = %path.display(), ?config, "config loaded");

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("kilog")
        .build()?;
    let handle = runtime.handle().clone();
    let _guard = runtime.enter();
    let token = if let Some(refresh) = kilog::auth::load_refresh_token()? {
        match runtime.block_on(kilog::auth::refresh_token_grant(&refresh)) {
            Ok(token) => {
                if let Some(next) = &token.refresh_token {
                    kilog::auth::save_refresh_token(next)?;
                }
                tracing::info!("session refreshed");
                Some(token)
            }
            Err(err) => {
                tracing::warn!(error = %err, "silent sign-in failed");
                None
            }
        }
    } else {
        None
    };

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Kilog")
            .with_inner_size([1120.0, 740.0])
            .with_min_inner_size([860.0, 540.0]),
        ..Default::default()
    };

    eframe::run_native(
        "kilog",
        options,
        Box::new(move |cc| {
            Ok(Box::new(kilog::ui::KilogApp::new(
                cc, handle, config, token,
            )))
        }),
    )?;

    Ok(())
}

use eframe::egui;

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let settings = kilog::config::load_or_create()?;
    let path = kilog::config::settings_path()?;
    tracing::info!(path = %path.display(), ?settings, "settings loaded");

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("kilog")
        .build()?;
    let handle = runtime.handle().clone();
    let _guard = runtime.enter();

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
        Box::new(move |cc| Ok(Box::new(kilog::ui::KilogApp::new(cc, handle)))),
    )?;

    Ok(())
}

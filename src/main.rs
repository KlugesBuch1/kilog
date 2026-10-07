use clap::Parser;
use eframe::egui;

use kilog::ui::Boot;

#[derive(Parser)]
#[command(name = "kilog")]
struct Args {
    /// Skip the network and install local placeholder Xbox headers for UI testing.
    #[arg(long)]
    dev_mock: bool,
    /// Same as --dev-mock.
    #[arg(long)]
    fast_boot: bool,
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();
    let config = kilog::config::AppConfig::load()?;
    let path = kilog::config::config_path()?;
    tracing::info!(path = %path.display(), ?config, "config loaded");

    let boot = if args.dev_mock || args.fast_boot {
        Boot::Mock
    } else {
        match kilog::auth::load_refresh_token() {
            Ok(Some(refresh)) => Boot::Restore(refresh),
            Ok(None) => Boot::Interactive,
            Err(err) => {
                tracing::warn!(error = %err, "saved session could not be read");
                Boot::Interactive
            }
        }
    };

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
        Box::new(move |cc| Ok(Box::new(kilog::ui::KilogApp::new(cc, handle, config, boot)))),
    )?;

    Ok(())
}

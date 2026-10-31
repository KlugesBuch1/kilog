#![cfg_attr(windows, windows_subsystem = "windows")]
use clap::Parser;
use eframe::egui;

use kilog::ui::Boot;

#[derive(Parser)]
#[command(name = "kilog")]
struct Args {
    #[arg(long)]
    dev_mock: bool,
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
            .with_title("kilog")
            .with_icon(app_icon())
            .with_decorations(false)
            .with_inner_size([1120.0, 740.0])
            .with_min_inner_size([860.0, 540.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };

    eframe::run_native(
        "kilog",
        options,
        Box::new(move |cc| Ok(Box::new(kilog::ui::KilogApp::new(cc, handle, config, boot)))),
    )?;

    Ok(())
}

fn app_icon() -> egui::IconData {
    const SVG: &[u8] = include_bytes!("../docs/assets/logo.svg");
    const SIZE: u32 = 256;

    let tree = resvg::usvg::Tree::from_data(SVG, &resvg::usvg::Options::default())
        .expect("logo.svg");
    let mut pixmap = resvg::tiny_skia::Pixmap::new(SIZE, SIZE).expect("app icon pixmap");
    let scale = SIZE as f32 / tree.size().width();
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );

    let mut rgba = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for pixel in pixmap.pixels() {
        let color = pixel.demultiply();
        rgba.extend_from_slice(&[color.red(), color.green(), color.blue(), color.alpha()]);
    }

    egui::IconData {
        rgba,
        width: SIZE,
        height: SIZE,
    }
}

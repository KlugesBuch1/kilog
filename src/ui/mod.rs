mod home;
mod page;
mod settings;
mod sidebar;
mod theme;

use crate::auth::MicrosoftOAuthResponse;
use crate::config::AppConfig;
use eframe::egui;
use home::{AuthState, AuthUpdate};
use page::Page;
use theme::{ACCENT, CANVAS};

pub struct KilogApp {
    runtime: tokio::runtime::Handle,
    page: Page,
    config: AppConfig,
    auth: AuthState,
    token: Option<MicrosoftOAuthResponse>,
    save_error: Option<String>,
    profile_rx: Option<
        tokio::sync::oneshot::Receiver<Result<crate::xbox::profile::PersonResponse, String>>,
    >,
    profile_error: Option<String>,
    auth_rx: Option<tokio::sync::mpsc::Receiver<AuthUpdate>>,
    auth_task: Option<tokio::task::JoinHandle<()>>,
    hwnd: isize,
    profile_pending: bool,
}

impl KilogApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        runtime: tokio::runtime::Handle,
        config: AppConfig,
        token: Option<crate::auth::MicrosoftOAuthResponse>,
    ) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        theme::apply(&cc.egui_ctx);
        if config.autostart_xbox_app {
            crate::utils::xbox_app::launch_xbox_app(config.start_xbox_app_hidden);
        }
        let profile_pending = token.is_some();
        Self {
            runtime,
            page: Page::Home,
            config,
            auth: AuthState::Disconnected,
            token,
            save_error: None,
            profile_rx: None,
            profile_error: None,
            auth_rx: None,
            auth_task: None,
            hwnd: 0,
            profile_pending,
        }
    }

    pub(super) fn start_login(&mut self, ctx: egui::Context) {
        let client_id = std::env::var("KILOG_OAUTH_CLIENT_ID").unwrap_or_default();

        if let Some(task) = self.auth_task.take() {
            task.abort();
        }
        self.profile_error = None;
        let hwnd = self.hwnd;
        let (tx, rx) = tokio::sync::mpsc::channel(8);
        self.auth_rx = Some(rx);
        self.auth_task = Some(self.runtime.spawn(async move {
            #[cfg(windows)]
            match crate::auth::request_wam_token(hwnd).await {
                Ok(token) => {
                    let _ = tx.send(AuthUpdate::Token(token)).await;
                    ctx.request_repaint();
                    return;
                }
                Err(err) => {
                    tracing::error!(error = %err, "windows sign-in unavailable, trying device code");
                }
            }
            run_device_login(tx, ctx, client_id).await;
        }));
    }

    pub(super) fn logout(&mut self) {
        if let Some(task) = self.auth_task.take() {
            task.abort();
        }
        self.auth_rx = None;
        self.token = None;
        if let Err(err) = crate::auth::clear_refresh_token() {
            tracing::error!(error = %err, "failed to clear refresh token");
        }
        self.auth = AuthState::Disconnected;
        self.profile_rx = None;
        self.profile_error = None;
    }

    fn poll_auth(&mut self) {
        let Some(rx) = self.auth_rx.as_mut() else {
            return;
        };
        while let Ok(update) = rx.try_recv() {
            match update {
                AuthUpdate::DeviceCode {
                    user_code,
                    verification_uri,
                } => {
                    self.auth = AuthState::Authenticating {
                        user_code,
                        verification_uri,
                    };
                }
                AuthUpdate::Token(token) => {
                    if let Some(refresh) = token.refresh_token.clone() {
                        if let Err(err) = crate::auth::save_refresh_token(&refresh) {
                            tracing::error!(error = %err, "failed to store refresh token");
                        }
                    }
                    self.token = Some(token);
                    self.profile_pending = true;
                }
                AuthUpdate::Failed(err) => {
                    self.profile_error = Some(err);
                    if !matches!(self.auth, AuthState::Authenticated { .. }) {
                        self.auth = AuthState::Disconnected;
                    }
                }
            }
        }
    }

    fn begin_profile_load(&mut self, ctx: egui::Context) {
        let Some(access_token) = self.token.as_ref().map(|token| token.access_token.clone()) else {
            return;
        };
        if self.profile_rx.is_some() || matches!(self.auth, AuthState::Authenticated { .. }) {
            return;
        }
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.profile_error = None;
        self.profile_rx = Some(rx);
        self.runtime.spawn(async move {
            let result = async {
                let authorization = crate::auth::request_xbox_live_token(&access_token).await?;
                crate::xbox::profile::fetch_me(&authorization).await
            }
            .await
            .map_err(|err| err.to_string());
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }

    fn poll_profile(&mut self) {
        let ready = self.profile_rx.as_mut().and_then(|rx| match rx.try_recv() {
            Ok(result) => Some(result),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty) => None,
            Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
                Some(Err("profile request was dropped".to_owned()))
            }
        });
        if let Some(result) = ready {
            self.profile_rx = None;
            match result {
                Ok(profile) => {
                    self.profile_error = None;
                    self.auth = AuthState::Authenticated { profile };
                }
                Err(err) => self.profile_error = Some(err),
            }
        }
    }

    fn capture_hwnd(&mut self, frame: &eframe::Frame) {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        if let Ok(handle) = frame.window_handle()
            && let RawWindowHandle::Win32(window) = handle.as_raw()
        {
            self.hwnd = window.hwnd.get();
        }
    }

    fn content(&mut self, ui: &mut egui::Ui) {
        ui.label(
            egui::RichText::new(self.page.title())
                .size(28.0)
                .strong()
                .color(theme::TEXT),
        );
        ui.add_space(10.0);

        let (bar, _) = ui.allocate_exact_size(egui::vec2(32.0, 2.0), egui::Sense::hover());
        ui.painter()
            .rect_filled(bar, egui::CornerRadius::same(1), ACCENT);

        ui.add_space(18.0);

        if self.page == Page::Home {
            self.home_page(ui);
        } else if self.page == Page::Settings {
            self.settings_form(ui);
        } else {
            ui.label(
                egui::RichText::new("Nothing here yet.")
                    .size(15.0)
                    .color(theme::MUTED),
            );
        }
    }
}

impl eframe::App for KilogApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        let [r, g, b, a] = CANVAS.to_array();
        [
            r as f32 / 255.0,
            g as f32 / 255.0,
            b as f32 / 255.0,
            a as f32 / 255.0,
        ]
    }

    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_auth();
        if self.profile_pending {
            self.profile_pending = false;
            self.begin_profile_load(ctx.clone());
        }
        self.poll_profile();
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.capture_hwnd(frame);
        egui::Panel::left("sidebar")
            .resizable(false)
            .show_separator_line(false)
            .default_size(228.0)
            .min_size(228.0)
            .max_size(228.0)
            .frame(
                egui::Frame::new()
                    .fill(theme::SIDEBAR)
                    .inner_margin(egui::Margin::symmetric(14, 18)),
            )
            .show(ui, |ui| self.sidebar(ui));

        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(CANVAS)
                    .inner_margin(egui::Margin::same(32)),
            )
            .show(ui, |ui| self.content(ui));
    }
}

async fn run_device_login(
    tx: tokio::sync::mpsc::Sender<AuthUpdate>,
    ctx: egui::Context,
    client_id: String,
) {
    let code = match crate::auth::request_device_code(&client_id).await {
        Ok(code) => code,
        Err(err) => {
            let _ = tx.send(AuthUpdate::Failed(err.to_string())).await;
            ctx.request_repaint();
            return;
        }
    };

    let _ = tx
        .send(AuthUpdate::DeviceCode {
            user_code: code.user_code,
            verification_uri: code.verification_uri,
        })
        .await;
    ctx.request_repaint();

    match crate::auth::poll_device_token(&client_id, &code.device_code, code.interval).await {
        Ok(token) => {
            let _ = tx.send(AuthUpdate::Token(token)).await;
        }
        Err(err) => {
            let _ = tx.send(AuthUpdate::Failed(err.to_string())).await;
        }
    }
    ctx.request_repaint();
}

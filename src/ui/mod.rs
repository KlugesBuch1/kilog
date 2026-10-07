mod games;
mod home;
mod page;
mod settings;
mod sidebar;
mod theme;
mod title_search;

use crate::auth::{MicrosoftOAuthResponse, XboxAuthorization};
use crate::config::AppConfig;
use crate::xbox::profile::PersonResponse;
use crate::xbox::titles::{GameFilter, TitlesList};
use eframe::egui;
use home::{AuthState, AuthUpdate};
use page::Page;
use theme::{ACCENT, CANVAS};

struct ProfileLoad {
    xbox: XboxAuthorization,
    profile: Result<PersonResponse, String>,
}

pub enum Boot {
    Restore(String),
    Interactive,
    Mock,
}

enum RestoreUpdate {
    Ready {
        token: MicrosoftOAuthResponse,
        xbox: XboxAuthorization,
        profile: Result<PersonResponse, String>,
    },
    SignedIn {
        token: MicrosoftOAuthResponse,
        error: String,
    },
    NeedsInteractive,
    Failed(String),
}

pub struct KilogApp {
    runtime: tokio::runtime::Handle,
    page: Page,
    config: AppConfig,
    auth: AuthState,
    token: Option<MicrosoftOAuthResponse>,
    save_error: Option<String>,
    profile_rx: Option<tokio::sync::oneshot::Receiver<Result<ProfileLoad, String>>>,
    profile_error: Option<String>,
    auth_rx: Option<tokio::sync::mpsc::Receiver<AuthUpdate>>,
    auth_task: Option<tokio::task::JoinHandle<()>>,
    hwnd: isize,
    profile_pending: bool,
    xbox: Option<XboxAuthorization>,
    restore_rx: Option<tokio::sync::oneshot::Receiver<RestoreUpdate>>,
    restore_task: Option<tokio::task::JoinHandle<()>>,
    restoring: bool,
    interactive_pending: bool,
    games_search: String,
    games_filter: GameFilter,
    titles: Option<TitlesList>,
    titles_error: Option<String>,
    titles_rx: Option<tokio::sync::oneshot::Receiver<(u64, Result<TitlesList, String>)>>,
    titles_epoch: u64,
    title_search: title_search::TitleSearch,
}

impl KilogApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        runtime: tokio::runtime::Handle,
        config: AppConfig,
        boot: Boot,
    ) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        theme::apply(&cc.egui_ctx);
        crate::debug_agent::log(
            "K",
            "ui/mod.rs:new",
            "startup does not launch the xbox app",
            serde_json::json!({ "launchedXboxApp": false }),
        );
        let mut app = Self {
            runtime,
            page: Page::Home,
            config,
            auth: AuthState::Disconnected,
            token: None,
            save_error: None,
            profile_rx: None,
            profile_error: None,
            auth_rx: None,
            auth_task: None,
            hwnd: 0,
            profile_pending: false,
            xbox: None,
            restore_rx: None,
            restore_task: None,
            restoring: false,
            interactive_pending: false,
            games_search: String::new(),
            games_filter: GameFilter::All,
            titles: None,
            titles_error: None,
            titles_rx: None,
            titles_epoch: 0,
            title_search: title_search::TitleSearch::new(),
        };
        match boot {
            Boot::Mock => app.apply_developer_mock(),
            Boot::Interactive => app.interactive_pending = true,
            Boot::Restore(refresh) => app.spawn_restore(cc.egui_ctx.clone(), refresh),
        }
        app
    }

    fn apply_developer_mock(&mut self) {
        self.token = Some(MicrosoftOAuthResponse {
            access_token: "dev-mock".into(),
            refresh_token: None,
            client_id: String::new(),
        });
        self.xbox = Some(XboxAuthorization::developer_mock());
        self.auth = AuthState::Authenticated {
            profile: PersonResponse::developer_preview(),
        };
        tracing::info!("developer mock session installed");
    }

    fn spawn_restore(&mut self, ctx: egui::Context, refresh: String) {
        self.cancel_restore();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.restore_rx = Some(rx);
        self.restoring = true;
        self.restore_task = Some(self.runtime.spawn(async move {
            let update = restore_saved_session(refresh).await;
            let _ = tx.send(update);
            ctx.request_repaint();
        }));
    }

    fn cancel_restore(&mut self) {
        if let Some(task) = self.restore_task.take() {
            task.abort();
        }
        self.restore_rx = None;
        self.restoring = false;
    }

    pub fn xbox_authorization(&self) -> Option<&XboxAuthorization> {
        self.xbox.as_ref()
    }

    pub(super) fn start_login(&mut self, ctx: egui::Context) {
        self.cancel_restore();

        if let Some(task) = self.auth_task.take() {
            task.abort();
        }
        self.profile_error = None;
        self.xbox = None;
        self.clear_titles();
        let (tx, rx) = tokio::sync::mpsc::channel(8);
        self.auth_rx = Some(rx);
        self.auth_task = Some(self.runtime.spawn(async move {
            let _ = tx.send(AuthUpdate::Waiting).await;
            ctx.request_repaint();
            match crate::auth::interactive_sign_in().await {
                Ok(token) => {
                    let _ = tx.send(AuthUpdate::Token(token)).await;
                }
                Err(err) => {
                    let _ = tx.send(AuthUpdate::Failed(err.to_string())).await;
                }
            }
            ctx.request_repaint();
        }));
    }

    pub(super) fn logout(&mut self) {
        self.cancel_restore();
        self.interactive_pending = false;
        if let Some(task) = self.auth_task.take() {
            task.abort();
        }
        self.auth_rx = None;
        self.token = None;
        self.xbox = None;
        self.clear_titles();
        self.games_search.clear();
        self.games_filter = GameFilter::All;
        self.title_search.clear();
        if let Err(err) = crate::auth::clear_refresh_token() {
            tracing::error!(error = %err, "failed to clear refresh token");
        }
        self.auth = AuthState::Disconnected;
        self.profile_rx = None;
        self.profile_error = None;
    }

    fn poll_restore(&mut self) {
        let ready = self.restore_rx.as_mut().and_then(|rx| match rx.try_recv() {
            Ok(update) => Some(update),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty) => None,
            Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
                Some(RestoreUpdate::Failed("session restore was dropped".into()))
            }
        });
        let Some(update) = ready else {
            return;
        };
        self.restore_rx = None;
        self.restore_task = None;
        self.restoring = false;
        match update {
            RestoreUpdate::Ready {
                token,
                xbox,
                profile,
            } => {
                tracing::info!("xbox live authorization ready");
                self.token = Some(token);
                self.xbox = Some(xbox);
                match profile {
                    Ok(profile) => {
                        self.profile_error = None;
                        self.auth = AuthState::Authenticated { profile };
                    }
                    Err(err) => self.profile_error = Some(err),
                }
            }
            RestoreUpdate::SignedIn { token, error } => {
                self.token = Some(token);
                self.profile_error = Some(error);
            }
            RestoreUpdate::NeedsInteractive => self.interactive_pending = true,
            RestoreUpdate::Failed(err) => self.profile_error = Some(err),
        }
    }

    fn poll_auth(&mut self) {
        let Some(rx) = self.auth_rx.as_mut() else {
            return;
        };
        while let Ok(update) = rx.try_recv() {
            match update {
                AuthUpdate::Waiting => {
                    self.auth = AuthState::Authenticating {
                        user_code: String::new(),
                        verification_uri: String::new(),
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
        let Some(token) = self.token.clone() else {
            return;
        };
        if token.access_token == "dev-mock"
            || self.profile_rx.is_some()
            || matches!(self.auth, AuthState::Authenticated { .. })
        {
            return;
        }
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.profile_error = None;
        self.profile_rx = Some(rx);
        self.runtime.spawn(async move {
            let result = match crate::auth::authorize_xbox_live(
                &token.access_token,
                &token.client_id,
                token.refresh_token.is_some(),
            )
            .await
            {
                Ok(xbox) => {
                    let profile = crate::xbox::profile::fetch_me(&xbox.authorization)
                        .await
                        .map_err(|err| err.to_string());
                    Ok(ProfileLoad { xbox, profile })
                }
                Err(err) => Err(err.to_string()),
            };
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
                Ok(load) => {
                    tracing::info!("xbox live authorization ready");
                    self.xbox = Some(load.xbox);
                    match load.profile {
                        Ok(profile) => {
                            self.profile_error = None;
                            self.auth = AuthState::Authenticated { profile };
                        }
                        Err(err) => self.profile_error = Some(err),
                    }
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
        } else if self.page == Page::Games {
            self.games_page(ui);
        } else if self.page == Page::TitleSearch {
            self.title_search_page(ui);
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
        self.poll_restore();
        self.poll_auth();
        if self.profile_pending {
            self.profile_pending = false;
            self.begin_profile_load(ctx.clone());
        }
        self.poll_profile();
        self.poll_titles();
        self.poll_title_search(ctx.clone());
        self.ensure_titles(ctx.clone());
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.capture_hwnd(frame);
        if self.interactive_pending {
            self.interactive_pending = false;
            let ctx = ui.ctx().clone();
            self.start_login(ctx);
        }
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

async fn restore_saved_session(refresh: String) -> RestoreUpdate {
    let token = match crate::auth::refresh_token_grant(&refresh).await {
        Ok(token) => token,
        Err(crate::error::Error::InvalidGrant) => {
            if let Err(err) = crate::auth::clear_refresh_token() {
                tracing::error!(error = %err, "failed to clear rejected session");
            }
            return RestoreUpdate::NeedsInteractive;
        }
        Err(err) => return RestoreUpdate::Failed(err.to_string()),
    };
    if let Some(next) = token.refresh_token.as_deref() {
        if let Err(err) = crate::auth::save_refresh_token(next) {
            tracing::error!(error = %err, "failed to store refresh token");
        }
    }
    let xbox = match crate::auth::authorize_xbox_live(
        &token.access_token,
        &token.client_id,
        token.refresh_token.is_some(),
    )
    .await
    {
        Ok(xbox) => xbox,
        Err(err) => {
            return RestoreUpdate::SignedIn {
                token,
                error: err.to_string(),
            };
        }
    };
    let profile = crate::xbox::profile::fetch_me(&xbox.authorization)
        .await
        .map_err(|err| err.to_string());
    RestoreUpdate::Ready {
        token,
        xbox,
        profile,
    }
}

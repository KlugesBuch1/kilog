use std::future::Future;
use std::path::{Path, PathBuf};

use eframe::egui::{self, Color32};

use super::KilogApp;
use super::home::AuthState;
use super::theme::{ACCENT, MUTED, TEXT};
use crate::xbox::profile::PersonResponse;

const ERROR: Color32 = Color32::from_rgb(232, 120, 128);
const MAX_BYTES: usize = 8 * 1024 * 1024;
const GAMERPIC_SIZE: u32 = 1080;

struct FoundPic {
    gamertag: String,
    bytes: Vec<u8>,
    id: u64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Busy {
    Idle,
    Search,
    Download,
    Steal,
}

enum Job {
    Search(Result<FoundPic, String>),
    Download(Result<PathBuf, String>),
    Steal(Result<Option<PersonResponse>, String>),
}

pub(super) struct GamerpicPage {
    gamertag: String,
    busy: Busy,
    error: Option<String>,
    notice: Option<String>,
    found: Option<FoundPic>,
    rx: Option<tokio::sync::oneshot::Receiver<(u64, Job)>>,
    task: Option<tokio::task::JoinHandle<()>>,
    epoch: u64,
    next_id: u64,
}

impl GamerpicPage {
    pub(super) fn new() -> Self {
        Self {
            gamertag: String::new(),
            busy: Busy::Idle,
            error: None,
            notice: None,
            found: None,
            rx: None,
            task: None,
            epoch: 0,
            next_id: 1,
        }
    }

    pub(super) fn clear(&mut self) {
        self.abort();
        let epoch = self.epoch;
        let next_id = self.next_id;
        *self = Self::new();
        self.epoch = epoch;
        self.next_id = next_id;
    }

    fn abort(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
        self.rx = None;
        self.busy = Busy::Idle;
        self.epoch = self.epoch.wrapping_add(1);
    }
}

impl KilogApp {
    pub(super) fn gamerpic_page(&mut self, ui: &mut egui::Ui) {
        if self.gamerpic.busy != Busy::Idle {
            ui.ctx().request_repaint();
        }

        let mut search = false;
        ui.horizontal(|ui| {
            let field =
                super::theme::text_field(ui, &mut self.gamerpic.gamertag, "Gamertag", 320.0);
            let enter = field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
            let clicked = accent_button(ui, "Search", super::theme::SEARCH_OUTER_HEIGHT).clicked();
            if self.gamerpic.busy == Busy::Idle && (enter || clicked) {
                search = true;
            }
            if self.gamerpic.busy != Busy::Idle {
                ui.spinner();
                ui.label(
                    egui::RichText::new(self.gamerpic.busy.label())
                        .size(14.0)
                        .color(MUTED),
                );
            }
        });
        ui.add_space(12.0);

        if search {
            let ctx = ui.ctx().clone();
            self.start_gamerpic_search(ctx);
        }

        if let Some(err) = &self.gamerpic.error {
            ui.label(egui::RichText::new(err).size(14.0).color(ERROR));
            ui.add_space(12.0);
        }

        let Some(found) = &self.gamerpic.found else {
            if self.gamerpic.error.is_none() {
                let message = if self.signed_in_xuid().is_none() {
                    "Sign in on Home, then search for a gamertag."
                } else {
                    "Search for a gamertag to see its gamerpic."
                };
                ui.label(egui::RichText::new(message).size(15.0).color(MUTED));
            }
            return;
        };

        ui.add(
            egui::Image::from_bytes(
                format!("bytes://gamerpic/{}", found.id),
                found.bytes.clone(),
            )
            .fit_to_exact_size(egui::vec2(148.0, 148.0))
            .corner_radius(16),
        );
        ui.add_space(12.0);
        ui.label(
            egui::RichText::new(&found.gamertag)
                .size(18.0)
                .strong()
                .color(TEXT),
        );
        ui.add_space(12.0);

        let mut download = false;
        let mut steal = false;
        if self.gamerpic.busy == Busy::Idle {
            ui.horizontal(|ui| {
                if accent_button(ui, "Download", super::theme::SEARCH_OUTER_HEIGHT).clicked() {
                    download = true;
                }
                if accent_button(ui, "Steal", super::theme::SEARCH_OUTER_HEIGHT).clicked() {
                    steal = true;
                }
            });
            ui.add_space(12.0);
        }

        if let Some(notice) = &self.gamerpic.notice {
            ui.label(egui::RichText::new(notice).size(14.0).color(MUTED));
        }

        if download {
            let ctx = ui.ctx().clone();
            self.start_gamerpic_download(ctx);
        } else if steal {
            let ctx = ui.ctx().clone();
            self.start_gamerpic_steal(ctx);
        }
    }

    pub(super) fn poll_gamerpic(&mut self) {
        let ready = self
            .gamerpic
            .rx
            .as_mut()
            .and_then(|rx| match rx.try_recv() {
                Ok(message) => Some(message),
                Err(tokio::sync::oneshot::error::TryRecvError::Empty) => None,
                Err(tokio::sync::oneshot::error::TryRecvError::Closed) => Some((
                    self.gamerpic.epoch,
                    Job::Search(Err("gamerpic request was dropped".into())),
                )),
            });
        let Some((epoch, job)) = ready else {
            return;
        };
        if epoch != self.gamerpic.epoch {
            return;
        }
        self.gamerpic.rx = None;
        self.gamerpic.task = None;
        self.gamerpic.busy = Busy::Idle;
        match job {
            Job::Search(Ok(found)) => {
                self.gamerpic.error = None;
                self.gamerpic.notice = None;
                self.gamerpic.found = Some(found);
            }
            Job::Search(Err(err)) => self.gamerpic.error = Some(err),
            Job::Download(Ok(path)) => {
                self.gamerpic.error = None;
                self.gamerpic.notice = Some(format!("Saved to {}", path.display()));
            }
            Job::Download(Err(err)) => self.gamerpic.error = Some(err),
            Job::Steal(Ok(profile)) => {
                self.gamerpic.error = None;
                self.gamerpic.notice = Some("Set as your gamerpic.".into());
                if let Some(profile) = profile
                    && matches!(self.auth, AuthState::Authenticated { .. })
                {
                    self.auth = AuthState::Authenticated { profile };
                }
            }
            Job::Steal(Err(err)) => self.gamerpic.error = Some(err),
        }
    }

    fn start_gamerpic_search(&mut self, ctx: egui::Context) {
        let gamertag = self.gamerpic.gamertag.trim().to_owned();
        if gamertag.is_empty() {
            return;
        }
        self.gamerpic.error = None;
        self.gamerpic.notice = None;
        if self.signed_in_xuid().is_none() {
            self.gamerpic.abort();
            self.gamerpic.found = None;
            self.gamerpic.error = Some("Sign in on Home to search for a gamerpic.".into());
            return;
        }
        let Some(authorization) = self.authorization() else {
            self.gamerpic.abort();
            self.gamerpic.found = None;
            self.gamerpic.error = Some("Xbox authorization is not ready.".into());
            return;
        };
        self.gamerpic.found = None;
        let id = self.gamerpic.next_id;
        self.gamerpic.next_id = self.gamerpic.next_id.wrapping_add(1);
        self.spawn_gamerpic(ctx, Busy::Search, async move {
            Job::Search(fetch_gamerpic(&authorization, &gamertag, id).await)
        });
    }

    fn start_gamerpic_download(&mut self, ctx: egui::Context) {
        let Some(found) = &self.gamerpic.found else {
            return;
        };
        let gamertag = found.gamertag.clone();
        let bytes = found.bytes.clone();
        self.gamerpic.error = None;
        self.gamerpic.notice = None;
        self.spawn_gamerpic(ctx, Busy::Download, async move {
            Job::Download(save_download(&gamertag, &bytes).map_err(|err| err.to_string()))
        });
    }

    fn start_gamerpic_steal(&mut self, ctx: egui::Context) {
        let Some(found) = &self.gamerpic.found else {
            return;
        };
        let bytes = found.bytes.clone();
        self.gamerpic.error = None;
        self.gamerpic.notice = None;
        if self.signed_in_xuid().is_none() {
            self.gamerpic.error = Some("Sign in on Home to set a gamerpic.".into());
            return;
        }
        let Some(authorization) = self.authorization() else {
            self.gamerpic.error = Some("Xbox authorization is not ready.".into());
            return;
        };
        self.spawn_gamerpic(ctx, Busy::Steal, async move {
            Job::Steal(steal_gamerpic(&authorization, &bytes).await)
        });
    }

    fn authorization(&self) -> Option<String> {
        self.xbox
            .as_ref()
            .map(|xbox| xbox.authorization.clone())
            .filter(|authorization| !authorization.trim().is_empty())
    }

    fn spawn_gamerpic<F>(&mut self, ctx: egui::Context, busy: Busy, work: F)
    where
        F: Future<Output = Job> + Send + 'static,
    {
        if let Some(task) = self.gamerpic.task.take() {
            task.abort();
        }
        self.gamerpic.epoch = self.gamerpic.epoch.wrapping_add(1);
        let epoch = self.gamerpic.epoch;
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.gamerpic.rx = Some(rx);
        self.gamerpic.busy = busy;
        self.gamerpic.task = Some(self.runtime.spawn(async move {
            let job = work.await;
            let _ = tx.send((epoch, job));
            ctx.request_repaint();
        }));
    }
}

impl Busy {
    fn label(self) -> &'static str {
        match self {
            Self::Idle => "",
            Self::Search => "Searching…",
            Self::Download => "Downloading…",
            Self::Steal => "Setting gamerpic…",
        }
    }
}

async fn fetch_gamerpic(authorization: &str, gamertag: &str, id: u64) -> Result<FoundPic, String> {
    let pic = crate::xbox::profile::gamerpic_for_gamertag(authorization, gamertag)
        .await
        .map_err(|err| err.to_string())?;
    let response = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|err| err.to_string())?
        .get(&pic.url)
        .send()
        .await
        .map_err(|err| err.to_string())?
        .error_for_status()
        .map_err(|err| err.to_string())?;
    let mime = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_owned();
    let bytes = response.bytes().await.map_err(|err| err.to_string())?;
    if bytes.is_empty() {
        return Err("gamerpic download was empty".into());
    }
    if bytes.len() > MAX_BYTES {
        return Err("gamerpic download was too large".into());
    }
    if !mime.is_empty() && !mime.starts_with("image/") {
        return Err(format!("gamerpic URL returned {mime}"));
    }
    Ok(FoundPic {
        gamertag: pic.gamertag,
        bytes: bytes.to_vec(),
        id,
    })
}

async fn steal_gamerpic(
    authorization: &str,
    bytes: &[u8],
) -> Result<Option<PersonResponse>, String> {
    let png = square_png(bytes)?;
    crate::xbox::profile::set_own_gamerpic(authorization, &png)
        .await
        .map_err(|err| err.to_string())?;
    Ok(crate::xbox::profile::fetch_me(authorization).await.ok())
}

fn square_png(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let source = image::load_from_memory(bytes).map_err(|err| err.to_string())?;
    let filter = if source.width().max(source.height()) <= 128 {
        image::imageops::FilterType::Nearest
    } else {
        image::imageops::FilterType::Lanczos3
    };
    let resized = source.resize_exact(GAMERPIC_SIZE, GAMERPIC_SIZE, filter);
    let mut png = Vec::new();
    resized
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|err| err.to_string())?;
    Ok(png)
}

fn save_download(gamertag: &str, bytes: &[u8]) -> Result<PathBuf, String> {
    let dir = directories::UserDirs::new()
        .and_then(|dirs| dirs.download_dir().map(Path::to_path_buf))
        .ok_or_else(|| "downloads folder not found".to_owned())?;
    std::fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    let path = unique_path(&dir, &file_stem(gamertag), image_extension(bytes));
    std::fs::write(&path, bytes).map_err(|err| err.to_string())?;
    Ok(path)
}

fn image_extension(bytes: &[u8]) -> &'static str {
    match bytes {
        [0x89, b'P', b'N', b'G', ..] => "png",
        [0xFF, 0xD8, 0xFF, ..] => "jpg",
        [b'G', b'I', b'F', ..] => "gif",
        [
            b'R',
            b'I',
            b'F',
            b'F',
            _,
            _,
            _,
            _,
            b'W',
            b'E',
            b'B',
            b'P',
            ..,
        ] => "webp",
        _ => "png",
    }
}

fn unique_path(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let candidate = dir.join(format!("{stem}.{ext}"));
    if !candidate.exists() {
        return candidate;
    }
    for index in 2..1000 {
        let path = dir.join(format!("{stem} ({index}).{ext}"));
        if !path.exists() {
            return path;
        }
    }
    dir.join(format!("{stem}-{}.{ext}", std::process::id()))
}

fn file_stem(gamertag: &str) -> String {
    let mut stem = String::new();
    for ch in gamertag.chars() {
        if matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') || ch.is_control() {
            stem.push('_');
        } else {
            stem.push(ch);
        }
    }
    let stem = stem.trim().trim_end_matches(['.', ' ']);
    if stem.is_empty() || is_reserved(stem) {
        "gamerpic".to_owned()
    } else {
        stem.to_owned()
    }
}

fn is_reserved(stem: &str) -> bool {
    let base = stem.split('.').next().unwrap_or(stem);
    matches!(
        base.to_ascii_uppercase().as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    )
}

fn accent_button(ui: &mut egui::Ui, label: &str, height: f32) -> egui::Response {
    let galley =
        ui.painter()
            .layout_no_wrap(label.to_owned(), egui::FontId::proportional(14.0), TEXT);
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(galley.size().x + 28.0, height),
        egui::Sense::click(),
    );
    let fill = if response.hovered() {
        ACCENT.gamma_multiply(1.12)
    } else {
        ACCENT
    };
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(super::theme::RADIUS), fill);
    ui.painter().galley(
        egui::pos2(
            rect.center().x - galley.size().x / 2.0,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        TEXT,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

#[cfg(test)]
mod tests {
    use super::{file_stem, square_png};

    #[test]
    fn file_stem_strips_windows_characters() {
        assert_eq!(file_stem(r#"a<b>:c"#), "a_b__c");
        assert_eq!(file_stem("  "), "gamerpic");
        assert_eq!(file_stem("CON"), "gamerpic");
        assert_eq!(file_stem("Major Nelson."), "Major Nelson");
    }

    #[test]
    fn small_images_become_square_pngs() {
        let image = image::RgbImage::from_pixel(4, 4, image::Rgb([10, 20, 30]));
        let mut bytes = Vec::new();
        image::DynamicImage::ImageRgb8(image)
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .unwrap();
        let png = square_png(&bytes).unwrap();
        let decoded = image::load_from_memory(&png).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (1080, 1080));
    }
}

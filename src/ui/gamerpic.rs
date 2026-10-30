use std::path::{Path, PathBuf};

use eframe::egui::{self, Color32};

use super::KilogApp;
use super::theme::{ACCENT, MUTED, TEXT};

const ERROR: Color32 = Color32::from_rgb(232, 120, 128);
const MAX_BYTES: usize = 8 * 1024 * 1024;

struct SavedPic {
    gamertag: String,
    path: PathBuf,
    bytes: Vec<u8>,
    id: u64,
}

pub(super) struct GamerpicPage {
    gamertag: String,
    busy: bool,
    error: Option<String>,
    saved: Option<SavedPic>,
    rx: Option<tokio::sync::oneshot::Receiver<(u64, Result<SavedPic, String>)>>,
    task: Option<tokio::task::JoinHandle<()>>,
    epoch: u64,
    next_id: u64,
}

impl GamerpicPage {
    pub(super) fn new() -> Self {
        Self {
            gamertag: String::new(),
            busy: false,
            error: None,
            saved: None,
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
        self.busy = false;
        self.epoch = self.epoch.wrapping_add(1);
    }
}

impl KilogApp {
    pub(super) fn gamerpic_page(&mut self, ui: &mut egui::Ui) {
        if self.gamerpic.busy {
            ui.ctx().request_repaint();
        }

        let mut submit = false;
        ui.horizontal(|ui| {
            let field =
                super::theme::text_field(ui, &mut self.gamerpic.gamertag, "Gamertag", 320.0);
            let enter = field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
            if self.gamerpic.busy {
                ui.spinner();
                ui.label(egui::RichText::new("Downloading…").size(14.0).color(MUTED));
            } else if enter
                || accent_button(ui, "Download", super::theme::SEARCH_OUTER_HEIGHT).clicked()
            {
                submit = true;
            }
        });
        ui.add_space(12.0);

        if submit {
            let ctx = ui.ctx().clone();
            self.start_gamerpic_download(ctx);
        }

        if let Some(err) = &self.gamerpic.error {
            ui.label(egui::RichText::new(err).size(14.0).color(ERROR));
            ui.add_space(12.0);
        }

        let Some(saved) = &self.gamerpic.saved else {
            if self.gamerpic.error.is_none() {
                let message = if self.signed_in_xuid().is_none() {
                    "Sign in on Home, then enter a gamertag."
                } else {
                    "Enter a gamertag to download its gamerpic."
                };
                ui.label(egui::RichText::new(message).size(15.0).color(MUTED));
            }
            return;
        };
        ui.add(
            egui::Image::from_bytes(
                format!("bytes://gamerpic/{}", saved.id),
                saved.bytes.clone(),
            )
            .fit_to_exact_size(egui::vec2(148.0, 148.0))
            .corner_radius(16),
        );
        ui.add_space(12.0);
        ui.label(
            egui::RichText::new(&saved.gamertag)
                .size(18.0)
                .strong()
                .color(TEXT),
        );
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(format!("Saved to {}", saved.path.display()))
                .size(14.0)
                .color(MUTED),
        );
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
                    Err("gamerpic download was dropped".into()),
                )),
            });
        let Some((epoch, result)) = ready else {
            return;
        };
        if epoch != self.gamerpic.epoch {
            return;
        }
        self.gamerpic.rx = None;
        self.gamerpic.task = None;
        self.gamerpic.busy = false;
        match result {
            Ok(saved) => {
                self.gamerpic.error = None;
                self.gamerpic.saved = Some(saved);
            }
            Err(err) => self.gamerpic.error = Some(err),
        }
    }

    fn start_gamerpic_download(&mut self, ctx: egui::Context) {
        let gamertag = self.gamerpic.gamertag.trim().to_owned();
        if gamertag.is_empty() {
            return;
        }
        self.gamerpic.error = None;
        if self.signed_in_xuid().is_none() {
            self.gamerpic.abort();
            self.gamerpic.error = Some("Sign in on Home to download a gamerpic.".into());
            return;
        }
        let Some(authorization) = self.xbox.as_ref().map(|xbox| xbox.authorization.clone()) else {
            self.gamerpic.abort();
            self.gamerpic.error = Some("Xbox authorization is not ready.".into());
            return;
        };
        if let Some(task) = self.gamerpic.task.take() {
            task.abort();
        }
        self.gamerpic.epoch = self.gamerpic.epoch.wrapping_add(1);
        let epoch = self.gamerpic.epoch;
        let id = self.gamerpic.next_id;
        self.gamerpic.next_id = self.gamerpic.next_id.wrapping_add(1);
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.gamerpic.rx = Some(rx);
        self.gamerpic.busy = true;
        self.gamerpic.task = Some(self.runtime.spawn(async move {
            let result = download_gamerpic(&authorization, &gamertag, id).await;
            let _ = tx.send((epoch, result));
            ctx.request_repaint();
        }));
    }
}

async fn download_gamerpic(
    authorization: &str,
    gamertag: &str,
    id: u64,
) -> Result<SavedPic, String> {
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
    let ext = image_extension(&mime, &bytes);
    let path = save_download(&pic.gamertag, ext, &bytes).map_err(|err| err.to_string())?;
    Ok(SavedPic {
        gamertag: pic.gamertag,
        path,
        bytes: bytes.to_vec(),
        id,
    })
}

fn image_extension(mime: &str, bytes: &[u8]) -> &'static str {
    match mime {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        "image/gif" => "gif",
        _ => match bytes {
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
        },
    }
}

fn save_download(gamertag: &str, ext: &str, bytes: &[u8]) -> Result<PathBuf, String> {
    let dir = directories::UserDirs::new()
        .and_then(|dirs| dirs.download_dir().map(Path::to_path_buf))
        .ok_or_else(|| "downloads folder not found".to_owned())?;
    std::fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    let path = unique_path(&dir, &file_stem(gamertag), ext);
    std::fs::write(&path, bytes).map_err(|err| err.to_string())?;
    Ok(path)
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
    use super::file_stem;

    #[test]
    fn file_stem_strips_windows_characters() {
        assert_eq!(file_stem(r#"a<b>:c"#), "a_b__c");
        assert_eq!(file_stem("  "), "gamerpic");
        assert_eq!(file_stem("CON"), "gamerpic");
        assert_eq!(file_stem("Major Nelson."), "Major Nelson");
    }
}

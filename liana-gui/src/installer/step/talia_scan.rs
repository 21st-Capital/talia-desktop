//! Talia Desktop: adding an existing vault by holding its QR code up to the
//! camera, such as the code Talia on the iPhone shows (decided 2026-11-15).
//!
//! What the code holds is handed to Liana's own backup import, exactly as if
//! the client had opened it as a file: a wallet backup with its names and
//! labels, a descriptor, or an encrypted backup that a signing device then
//! opens. Nothing is parsed here but the QR frames themselves.

use std::{io::Write, path::PathBuf};

use iced::{
    widget::{column, image, Space},
    Alignment, Length, Subscription, Task,
};
use liana::miniscript::bitcoin::Network;
use liana_ui::{
    component::{button, modal, text::new},
    theme,
    widget::{modal::Modal, ColumnExt, Container, Element},
};

use crate::{
    bbqr,
    export::{from_backup, ImportExportMessage, Progress},
    installer::message::Message,
    t,
    talia_camera::{self, Access, Event},
};

/// The preview's size in the window.
const PREVIEW: (f32, f32) = (480.0, 270.0);

#[derive(Debug, Clone)]
pub enum ScanMessage {
    Open,
    Access(Access),
    Camera(Event),
    OpenSettings,
    /// What Liana's backup import made of the code, or why it could not.
    Read(Result<Vec<Progress>, String>),
    Again,
    Close,
}

impl From<ScanMessage> for Message {
    fn from(message: ScanMessage) -> Self {
        Message::TaliaScan(message)
    }
}

enum Phase {
    Asking,
    Denied,
    NoCamera,
    Scanning {
        preview: Option<image::Handle>,
        /// Frames read and frames in the code, for a moving code.
        parts: Option<(usize, usize)>,
    },
    Reading,
    Failed(String),
}

pub struct ScanWindow {
    phase: Phase,
    joiner: bbqr::Joiner,
}

impl ScanWindow {
    pub fn open() -> (Self, Task<Message>) {
        (
            Self {
                phase: Phase::Asking,
                joiner: bbqr::Joiner::default(),
            },
            ask(),
        )
    }

    /// The next task, and whether the window stays open.
    pub fn update(&mut self, message: ScanMessage, network: Network) -> (Task<Message>, bool) {
        match message {
            ScanMessage::Open => {}
            ScanMessage::Close => return (Task::none(), false),
            ScanMessage::OpenSettings => talia_camera::open_privacy_settings(),
            ScanMessage::Again => {
                self.joiner = bbqr::Joiner::default();
                self.phase = Phase::Asking;
                return (ask(), true);
            }
            ScanMessage::Access(access) => {
                self.phase = match access {
                    Access::Granted => Phase::Scanning {
                        preview: None,
                        parts: None,
                    },
                    Access::Denied => Phase::Denied,
                    Access::NoCamera => Phase::NoCamera,
                }
            }
            ScanMessage::Camera(Event::Failed(e)) => self.phase = Phase::Failed(e),
            ScanMessage::Camera(Event::Frame { preview, codes }) => {
                if let Phase::Scanning { preview: shown, .. } = &mut self.phase {
                    *shown = Some(preview);
                    for code in codes {
                        if let Some(content) = self.take(code) {
                            self.phase = Phase::Reading;
                            return (
                                Task::perform(read(content, network), |result| {
                                    ScanMessage::Read(result).into()
                                }),
                                true,
                            );
                        }
                    }
                }
            }
            ScanMessage::Read(Ok(progress)) => {
                // The import step takes it from here, as from a file.
                return (
                    Task::batch(progress.into_iter().map(|p| {
                        Task::done(Message::ImportExport(ImportExportMessage::Progress(p)))
                    })),
                    false,
                );
            }
            ScanMessage::Read(Err(e)) => {
                tracing::warn!("Scanned QR code not imported: {}", e);
                self.phase = Phase::Failed(t!("talia-scan-not-a-vault"));
            }
        }
        (Task::none(), true)
    }

    /// The whole content once a code is read: a plain code at once, a moving
    /// code once every frame is in.
    fn take(&mut self, code: String) -> Option<Vec<u8>> {
        if !bbqr::is_frame(&code) {
            // A plain code, such as a descriptor; not while a moving one is
            // half read.
            return self.joiner.is_empty().then(|| code.into_bytes());
        }
        let added = match self.joiner.add(&code) {
            // Another moving code was held up: start again with it.
            Err(bbqr::Error::OtherCode) => {
                self.joiner = bbqr::Joiner::default();
                self.joiner.add(&code)
            }
            added => added,
        };
        match added {
            Ok(parts) => {
                if let Phase::Scanning { parts: shown, .. } = &mut self.phase {
                    *shown = Some(parts);
                }
                match self.joiner.result()? {
                    Ok(content) => Some(content),
                    Err(e) => {
                        tracing::warn!("Scanned QR code not readable: {}", e);
                        self.joiner = bbqr::Joiner::default();
                        None
                    }
                }
            }
            Err(e) => {
                tracing::debug!("QR frame skipped: {}", e);
                None
            }
        }
    }

    /// The camera, only while it is needed.
    pub fn subscription(&self) -> Subscription<Message> {
        if matches!(self.phase, Phase::Scanning { .. }) {
            Subscription::run(talia_camera::events).map(|e| ScanMessage::Camera(e).into())
        } else {
            Subscription::none()
        }
    }

    pub fn view<'a>(&'a self, content: Element<'a, Message>) -> Element<'a, Message> {
        let body: Element<'a, Message> = match &self.phase {
            Phase::Asking => caption(t!("talia-scan-asking")),
            Phase::NoCamera => caption(t!("talia-scan-no-camera")),
            Phase::Reading => caption(t!("talia-scan-reading")),
            Phase::Denied => column![
                caption(t!("talia-scan-denied")),
                button::secondary(None, t!("talia-scan-open-settings"))
                    .on_press(ScanMessage::OpenSettings.into()),
            ]
            .spacing(15)
            .into(),
            Phase::Failed(e) => column![
                new::caption(e.clone()).style(theme::text::error),
                button::secondary(None, t!("talia-scan-again")).on_press(ScanMessage::Again.into()),
            ]
            .spacing(15)
            .into(),
            Phase::Scanning { preview, parts } => {
                let picture: Element<'a, Message> = match preview {
                    Some(handle) => image::Image::new(handle.clone())
                        .width(Length::Fixed(PREVIEW.0))
                        .height(Length::Fixed(PREVIEW.1))
                        .into(),
                    None => Space::new()
                        .width(Length::Fixed(PREVIEW.0))
                        .height(Length::Fixed(PREVIEW.1))
                        .into(),
                };
                column![Container::new(picture).style(theme::card::simple)]
                    .push_maybe(parts.filter(|(_, total)| *total > 1).map(|(have, total)| {
                        new::caption(t!("talia-scan-parts", have = have, total = total))
                            .style(theme::text::secondary)
                    }))
                    .spacing(10)
                    .align_x(Alignment::Center)
                    .width(Length::Fill)
                    .into()
            }
        };
        Modal::new(
            content,
            modal::modal_view(
                Some(t!("talia-scan-title")),
                None,
                Some(ScanMessage::Close.into()),
                modal::ModalWidth::L,
                body,
            ),
        )
        .on_blur(Some(ScanMessage::Close.into()))
        .into()
    }
}

fn ask() -> Task<Message> {
    Task::perform(talia_camera::authorize(), |access| {
        ScanMessage::Access(access).into()
    })
}

fn caption<'a>(text: String) -> Element<'a, Message> {
    new::caption(text).style(theme::text::secondary).into()
}

/// Hands the code's content to Liana's backup import through a file only
/// this user may read, removed as soon as it is read.
async fn read(content: Vec<u8>, network: Network) -> Result<Vec<Progress>, String> {
    let path = scratch_file(&content).map_err(|e| e.to_string())?;
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let result = from_backup(&sender, path.clone(), network).await;
    let _ = std::fs::remove_file(&path);
    result.map_err(|e| e.to_string())?;
    drop(sender);
    let mut progress = Vec::new();
    while let Ok(p) = receiver.try_recv() {
        if matches!(p, Progress::WalletFromBackup(_) | Progress::EncryptedFile(_)) {
            progress.push(p);
        }
    }
    if progress.is_empty() {
        return Err("Nothing imported".into());
    }
    Ok(progress)
}

fn scratch_file(content: &[u8]) -> std::io::Result<PathBuf> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let path = std::env::temp_dir().join(format!(
        "talia-desktop-scan-{}-{}",
        std::process::id(),
        nanos
    ));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&path)?;
    file.write_all(content)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BACKUP: &str = include_str!("../../../test_assets/liana-backup-2025-06-23T13-23-54.json");

    #[tokio::test]
    async fn a_scanned_backup_goes_through_liana_s_own_import() {
        // As Talia on the iPhone shows it: the wallet backup, in frames.
        let frames = bbqr::encode(BACKUP.as_bytes(), bbqr::FileType::Json).unwrap();
        let mut window = ScanWindow {
            phase: Phase::Scanning {
                preview: None,
                parts: None,
            },
            joiner: bbqr::Joiner::default(),
        };
        let mut content = None;
        for frame in frames.iter().rev() {
            assert!(content.is_none());
            content = window.take(frame.clone());
        }
        let content = content.expect("every frame read");
        assert_eq!(content, BACKUP.as_bytes());
        let progress = read(content, Network::Signet).await.unwrap();
        assert!(matches!(
            progress.as_slice(),
            [Progress::WalletFromBackup(_)]
        ));
        // No scratch file is left behind.
        let left = std::fs::read_dir(std::env::temp_dir())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with(&format!("talia-desktop-scan-{}-", std::process::id()))
            })
            .count();
        assert_eq!(left, 0);
    }

    #[tokio::test]
    async fn a_plain_code_that_is_no_vault_is_refused() {
        assert!(read(b"hello".to_vec(), Network::Signet).await.is_err());
    }
}

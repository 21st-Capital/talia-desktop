//! Talia Desktop: the wallet backup as a moving QR code for Talia on the
//! iPhone (decided 2026-11-15). The settings entry and its window; the window
//! shows a title and the code, nothing else.

use iced::{widget::qr_code, Alignment, Length};

use liana_ui::{
    component::{
        badge::Tile,
        button::EntryWidth,
        list,
        modal,
        text::new,
    },
    theme,
    widget::{Column, Container, Element},
};

use crate::{
    app::view::message::{Message, SettingsMessage},
    t,
};

/// The code's side: dense frames still scan from a phone held at arm's length.
const QR_SIZE: f32 = 460.0;

#[derive(Debug, Clone)]
pub enum TaliaBackupQrMessage {
    Open,
    /// The frames, or why there are none.
    Built(Result<Vec<String>, String>),
    /// The next frame.
    Tick,
    Close,
}

impl From<TaliaBackupQrMessage> for Message {
    fn from(message: TaliaBackupQrMessage) -> Self {
        Message::Settings(SettingsMessage::TaliaBackupQr(message))
    }
}

/// The entry beside "Back up wallet".
pub fn entry<'a>() -> Element<'a, Message> {
    list::entry_section(
        Tile::Backup,
        t!("talia-backup-qr"),
        Some(t!("talia-backup-qr-description")),
        EntryWidth::Fill,
        Some(TaliaBackupQrMessage::Open.into()),
    )
}

/// What the window shows.
pub enum Shown<'a> {
    Preparing,
    Frame(&'a qr_code::Data),
    Failed(&'a str),
}

pub fn window<'a>(shown: Shown<'a>) -> Element<'a, Message> {
    let content: Element<'a, Message> = match shown {
        Shown::Preparing => new::caption(t!("talia-backup-qr-preparing"))
            .style(theme::text::secondary)
            .into(),
        Shown::Failed(error) => new::caption(error.to_string())
            .style(theme::text::error)
            .into(),
        Shown::Frame(data) => Column::new()
            .push(
                Container::new(qr_code::QRCode::<theme::Theme>::new(data).total_size(QR_SIZE))
                    .padding(10),
            )
            .align_x(Alignment::Center)
            .width(Length::Fill)
            .into(),
    };
    modal::modal_view(
        Some(t!("talia-backup-qr-title")),
        None,
        Some(TaliaBackupQrMessage::Close.into()),
        modal::ModalWidth::L,
        content,
    )
}

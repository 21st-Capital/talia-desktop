use crate::{component::text::P1_SIZE, widget::*};
use iced::{alignment, Font, Length};
use iced_core::text::LineHeight;

pub const ICON_SIZE_L: u16 = 32;
pub const ICON_SIZE_M: u16 = 24;
pub const ICON_SIZE_S: u16 = 16;

// Talia Desktop: the Talia iPhone app's icons (decided 2026-11-14). One font,
// static/icons/talia-icons: the Tabler outline set the app draws (MIT), cut
// down to the glyphs below, plus the app's own key, vault and refresh marks.
// Every function keeps Liana's name, so no caller changes; the glyph behind
// it is ours. static/icons/talia-icons/build_font.py rebuilds the font.
const TALIA_ICONS: Font = Font::with_name("talia-icons");
pub(crate) const FONT_ICON_WIDTH: f32 = 20.0;

/// The app's own drawn key (KeyIcon), the one key icon everywhere.
const KEY: char = '\u{E000}';
/// The app's Vault tab icon (VaultIcon): a safe.
const VAULT: char = '\u{E001}';
/// The app's refresh arrow, its head at the top left.
const REFRESH: char = '\u{E002}';

fn font_icon<'a>(font: Font, unicode: char) -> Text<'a> {
    Text::new(unicode.to_string())
        .font(font)
        .width(Length::Fixed(FONT_ICON_WIDTH))
        .align_x(alignment::Horizontal::Center)
        .size(P1_SIZE)
}

fn font_icon_no_padding<'a>(font: Font, unicode: char) -> Text<'a> {
    Text::new(unicode.to_string())
        .font(font)
        .align_x(alignment::Horizontal::Center)
        .size(P1_SIZE)
        .line_height(LineHeight::Relative(1.0))
}

fn talia_icon<'a>(unicode: char) -> Text<'a> {
    font_icon(TALIA_ICONS, unicode)
}

fn talia_icon_no_padding<'a>(unicode: char) -> Text<'a> {
    font_icon_no_padding(TALIA_ICONS, unicode)
}

/// Tabler: x.
pub fn cross_icon<'a>() -> Text<'a> {
    talia_icon_no_padding('\u{EB55}')
}

/// Tabler: x.
pub fn big_cross_icon<'a>() -> Text<'a> {
    talia_icon('\u{EB55}')
}

/// Tabler: arrow-down.
pub fn arrow_down() -> Text<'static> {
    talia_icon('\u{EA16}')
}

/// Tabler: arrow-right.
pub fn arrow_right() -> Text<'static> {
    talia_icon('\u{EA1F}')
}

/// Tabler: corner-down-right.
pub fn arrow_return_right() -> Text<'static> {
    talia_icon('\u{EA7D}')
}

/// Tabler: chevron-right.
pub fn chevron_right() -> Text<'static> {
    talia_icon('\u{EA61}')
}

pub fn recovery_icon() -> Text<'static> {
    talia_icon(KEY)
}

/// Tabler: plug.
pub fn plug_icon() -> Text<'static> {
    talia_icon('\u{EBD9}')
}

pub fn reload_icon<'a>() -> Text<'a> {
    talia_icon(REFRESH)
}

/// Tabler: download.
pub fn import_icon<'a>() -> Text<'a> {
    talia_icon('\u{EA96}')
}

/// Tabler: wallet.
pub fn wallet_icon<'a>() -> Text<'a> {
    talia_icon('\u{EB75}')
}

/// Tabler: currency-bitcoin.
pub fn bitcoin_icon<'a>() -> Text<'a> {
    talia_icon('\u{EBAB}')
}

/// Tabler: box.
pub fn block_icon<'a>() -> Text<'a> {
    talia_icon('\u{EA45}')
}

/// A solid dot, drawn for this font.
pub fn dot_icon() -> Text<'static> {
    talia_icon('\u{E003}')
}

/// Tabler: user.
pub fn person_icon<'a>() -> Text<'a> {
    talia_icon('\u{EB4D}')
}

/// Tabler: info-circle.
pub fn tooltip_icon<'a>() -> Text<'a> {
    talia_icon('\u{EAC5}')
}

/// Tabler: plus.
pub fn plus_icon<'a>() -> Text<'a> {
    talia_icon('\u{EB0B}')
}

/// Tabler: minus.
pub fn minus_icon<'a>() -> Text<'a> {
    talia_icon('\u{EAF2}')
}

/// Tabler: alert-triangle.
pub fn warning_icon<'a>() -> Text<'a> {
    talia_icon('\u{EA06}')
}

/// Tabler: shield.
pub fn shield_icon<'a>() -> Text<'a> {
    talia_icon('\u{EB24}')
}

/// Tabler: lock.
pub fn lock_icon<'a>() -> Text<'a> {
    talia_icon('\u{EAE2}')
}

/// Tabler: alert-triangle, solid.
pub fn warning_fill_icon() -> Text<'static> {
    talia_icon('\u{E011}')
}

/// Tabler: cpu.
pub fn chip_icon<'a>() -> Text<'a> {
    talia_icon('\u{EF8E}')
}

/// Tabler: trash.
pub fn trash_icon() -> Text<'static> {
    talia_icon('\u{EB41}')
}

/// Tabler: chevron-right.
pub fn collapse_icon<'a>() -> Text<'a> {
    talia_icon('\u{EA61}')
}

/// Tabler: chevron-down.
pub fn collapsed_icon<'a>() -> Text<'a> {
    talia_icon('\u{EA5F}')
}

/// Tabler: chevron-down.
pub fn down_icon() -> Text<'static> {
    talia_icon('\u{EA5F}')
}

/// Tabler: chevron-up.
pub fn up_icon() -> Text<'static> {
    talia_icon('\u{EA62}')
}

/// Tabler: network.
pub fn network_icon<'a>() -> Text<'a> {
    talia_icon('\u{F09F}')
}

/// Tabler: chevron-left.
pub fn previous_icon<'a>() -> Text<'a> {
    talia_icon('\u{EA60}')
}

/// Tabler: check.
pub fn check_icon<'a>() -> Text<'a> {
    talia_icon('\u{EA5E}')
}

/// Tabler: check.
pub fn check_mark_icon<'a>() -> Text<'a> {
    talia_icon_no_padding('\u{EA5E}')
}

pub fn round_key_icon<'a>() -> Text<'a> {
    talia_icon(KEY)
}

/// Tabler: file-export.
pub fn backup_icon<'a>() -> Text<'a> {
    talia_icon('\u{EDE9}')
}

/// Tabler: file-import.
pub fn restore_icon<'a>() -> Text<'a> {
    talia_icon('\u{EDEA}')
}

/// Tabler: tool.
pub fn wrench_icon<'a>() -> Text<'a> {
    talia_icon('\u{EB40}')
}

/// Tabler: link.
pub fn link_icon() -> Text<'static> {
    talia_icon('\u{EADE}')
}

/// Tabler: clipboard.
pub fn paste_icon<'a>() -> Text<'a> {
    talia_icon('\u{EA6F}')
}

/// Tabler: usb.
pub fn usb_icon<'a>() -> Text<'a> {
    talia_icon('\u{F00C}')
}

/// Tabler: device-usb.
pub fn usb_drive_icon<'a>() -> Text<'a> {
    talia_icon('\u{FC59}')
}

/// Tabler: server.
pub fn hdd_icon() -> Text<'static> {
    talia_icon('\u{EB1F}')
}

/// Tabler: login-2.
pub fn enter_box_icon<'a>() -> Text<'a> {
    talia_icon('\u{FC76}')
}

/// Tabler: list-details.
pub fn collection_icon() -> Text<'static> {
    talia_icon('\u{EF40}')
}

/// Tabler: building.
pub fn org_icon<'a>() -> Text<'a> {
    talia_icon('\u{EA4F}')
}

/// Tabler: coins.
pub fn coins_icon() -> Text<'static> {
    talia_icon('\u{F65D}')
}

/// Tabler: arrow-down, the app's received arrow.
pub fn receive_icon() -> Text<'static> {
    talia_icon('\u{EA16}')
}

/// Tabler: arrow-up, the app's sent arrow.
pub fn send_icon() -> Text<'static> {
    talia_icon('\u{EA25}')
}

/// Tabler: settings.
pub fn settings_icon() -> Text<'static> {
    talia_icon('\u{EB20}')
}

/// Tabler: clock.
pub fn clock_icon() -> Text<'static> {
    talia_icon('\u{EA70}')
}

/// Tabler: clock, solid.
pub fn clock_fill_icon() -> Text<'static> {
    talia_icon('\u{E010}')
}

/// Tabler: pencil.
pub fn edit_icon_padding<'a>() -> Text<'a> {
    talia_icon('\u{EB04}')
}

/// Tabler: pencil.
pub fn edit_icon<'a>() -> Text<'a> {
    talia_icon_no_padding('\u{EB04}')
}

/// Tabler: qrcode.
pub fn qr_icon<'a>() -> Text<'a> {
    talia_icon('\u{EB11}')
}

/// Tabler: copy.
pub fn copy_icon<'a>() -> Text<'a> {
    talia_icon_no_padding('\u{EA7A}')
}

/// Tabler: file-export.
pub fn file_icon<'a>() -> Text<'a> {
    talia_icon('\u{EDE9}')
}

/// Tabler: repeat.
pub fn arrow_repeat() -> Text<'static> {
    talia_icon('\u{EB72}')
}

pub fn home_icon() -> Text<'static> {
    talia_icon(VAULT)
}

pub fn key_icon<'a>() -> Text<'a> {
    talia_icon(KEY)
}

/// Tabler: history.
pub fn history_icon() -> Text<'static> {
    talia_icon('\u{EBEA}')
}

/// Tabler: clipboard.
pub fn clipboard_icon<'a>() -> Text<'a> {
    talia_icon('\u{EA6F}')
}

/// Tabler: circle-check.
pub fn circle_check_icon() -> Text<'static> {
    talia_icon('\u{EA67}')
}

/// Tabler: circle-x.
pub fn circle_cross_icon() -> Text<'static> {
    talia_icon('\u{EA6A}')
}

/// Tabler: scale.
pub fn scale_icon<'a>() -> Text<'a> {
    talia_icon('\u{EBC2}')
}

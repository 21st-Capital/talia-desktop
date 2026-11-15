// Talia Desktop: the Talia iPhone app's type (decided 2026-11-14). Figtree for
// text, Bricolage Grotesque for headings and every bold weight, as the iPhone
// app's themedStyles does (600+ weight or large size = Bricolage). The constant
// names are Liana's, kept so upstream code needs no change; the faces behind
// them are ours. Both families are SIL OFL 1.1 (static/fonts/OFL-*.txt).
use iced::{
    font::{Family, Stretch, Weight},
    Font,
};
use std::borrow::Cow;

pub const BOLD: Font = Font {
    family: Family::Name("Bricolage Grotesque"),
    weight: Weight::Bold,
    style: iced::font::Style::Normal,
    stretch: Stretch::Normal,
};

pub const MEDIUM: Font = Font {
    family: Family::Name("Figtree"),
    weight: Weight::Medium,
    style: iced::font::Style::Normal,
    stretch: Stretch::Normal,
};

pub const REGULAR: Font = Font::with_name("Figtree");

pub const MANROPE_BOLD: Font = Font {
    family: Family::Name("Bricolage Grotesque"),
    weight: Weight::Bold,
    style: iced::font::Style::Normal,
    stretch: Stretch::Normal,
};

pub const MANROPE_MEDIUM: Font = Font {
    family: Family::Name("Bricolage Grotesque"),
    weight: Weight::Medium,
    style: iced::font::Style::Normal,
    stretch: Stretch::Normal,
};

pub const MANROPE_REGULAR: Font = Font::with_name("Bricolage Grotesque");

pub const MANROPE_SEMIBOLD: Font = Font {
    family: Family::Name("Bricolage Grotesque"),
    weight: Weight::Semibold,
    style: iced::font::Style::Normal,
    stretch: Stretch::Normal,
};

pub const IBM_BOLD_BYTES: &[u8] = include_bytes!("../static/fonts/BricolageGrotesque-Bold.ttf");
pub const IBM_MEDIUM_BYTES: &[u8] = include_bytes!("../static/fonts/Figtree-Medium.ttf");
pub const IBM_REGULAR_BYTES: &[u8] = include_bytes!("../static/fonts/Figtree-Regular.ttf");

pub const MANROPE_REGULAR_BYTES: &[u8] =
    include_bytes!("../static/fonts/BricolageGrotesque-Regular.ttf");
pub const MANROPE_MEDIUM_BYTES: &[u8] =
    include_bytes!("../static/fonts/BricolageGrotesque-Medium.ttf");
pub const MANROPE_SEMIBOLD_BYTES: &[u8] =
    include_bytes!("../static/fonts/BricolageGrotesque-SemiBold.ttf");
pub const MANROPE_BOLD_BYTES: &[u8] = include_bytes!("../static/fonts/BricolageGrotesque-Bold.ttf");

pub const ICONEX_ICONS_BYTES: &[u8] = include_bytes!("../static/icons/iconex/iconex-icons.ttf");
pub const BOOTSTRAP_ICONS_BYTE: &[u8] = include_bytes!("../static/icons/bootstrap-icons.ttf");
pub const LIANA_ICONS_BYTES: &[u8] = include_bytes!("../static/icons/liana-icons/liana-icons.ttf");
// Talia Desktop: the iPhone app's icons (Tabler outline subset plus our own
// key and vault), MIT: static/icons/talia-icons/LICENSE-tabler-icons.txt.
pub const TALIA_ICONS_BYTES: &[u8] = include_bytes!("../static/icons/talia-icons/talia-icons.ttf");

pub fn load() -> Vec<Cow<'static, [u8]>> {
    vec![
        IBM_BOLD_BYTES.into(),
        IBM_MEDIUM_BYTES.into(),
        IBM_REGULAR_BYTES.into(),
        MANROPE_REGULAR_BYTES.into(),
        MANROPE_MEDIUM_BYTES.into(),
        MANROPE_SEMIBOLD_BYTES.into(),
        MANROPE_BOLD_BYTES.into(),
        // Bootstrap stays loaded for the vault graph's key glyph
        // (component/timeline.rs); the iconex and liana-icons fonts have no
        // user left and are no longer loaded.
        BOOTSTRAP_ICONS_BYTE.into(),
        TALIA_ICONS_BYTES.into(),
    ]
}

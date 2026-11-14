use iced::Color;

#[macro_export]
macro_rules! color {
    ($name:ident, $hex:expr) => {
        color!($name, $hex, 1.0);
    };
    ($name:ident, $hex:expr, $a:expr) => {
        pub const $name: iced::Color = iced::Color {
            r: (($hex >> 16) & 0xFF) as f32 / 255.0,
            g: (($hex >> 8) & 0xFF) as f32 / 255.0,
            b: ($hex & 0xFF) as f32 / 255.0,
            a: $a,
        };
    };
}

pub const BLACK: Color = iced::Color::BLACK;
pub const TRANSPARENT: Color = iced::Color::TRANSPARENT;
pub const WHITE: Color = iced::Color::WHITE;

color!(LIGHT_BLACK, 0x01120F);
color!(BUSINESS_BLACK, 0x0F172A);
color!(GREY_7, 0x23312D);
color!(GREY_6, 0x12221E);
color!(GREY_5, 0x1E2D29);
color!(GREY_4, 0x2D3A36);
color!(GREY_3, 0x738983);
color!(GREY_2, 0xC9D1CE);
color!(GREY_1, 0xE5E9E8);
color!(GREEN, 0x16C784);
color!(SUCCESS_GREEN, 0x16C784);
color!(TRANSPARENT_GREEN, 0x16C784, 0.3);
color!(RED, 0xFF4C4C);
color!(ORANGE, 0xF2A900);
color!(BLUE, 0x3A91FF);
color!(FINGERPRINT_BACKGROUND, 0x062B24);
color!(FINGERPRINT_BORDER, 0x124A38);
color!(FINGERPRINT_TEXT, 0x16C784);
color!(FORM_PLACEHOLDER, 0x738983);
color!(CHECKBOX_BORDER, 0x556762);

// BUSINESS
color!(BUSINESS_BLUE, 0x00BDFF);
color!(BUSINESS_BLUE_50, 0x80DEFF);
color!(BUSINESS_BLUE_DARK, 0x0099CC);
color!(BUSINESS_LIGHT_BLUE, 0xE9F4FF);
color!(BUSINESS_BLUE_SECTION, 0xC1E9F7);
color!(BUSINESS_PILL_SIMPLE, 0x7E889F);
color!(BUSINESS_STEP_TRACK, 0xE2E8F0);
color!(BUSINESS_STEP_LABEL, 0x94A3B8);
color!(TRANSPARENT_BUSINESS_BLUE, 0x00BFFF, 0.15);
color!(LIGHT_BLUE_TINT, 0xE5F5FF);
color!(SOFT_BLUE, 0x66D4FF);
color!(DARK_GREEN, 0x007A33);
color!(LIGHT_BG, 0xF8F8F8);
color!(LIGHT_BG_SECONDARY, 0xE5E5E5);
color!(LIGHT_BG_TERTIARY, 0xD5D5D5);
color!(DARK_TEXT_SECONDARY, 0x101213);
color!(DARK_TEXT_TERTIARY, 0x6B6B6B);
color!(LIGHT_BORDER, 0xA5A5A5);
color!(LIGHT_BORDER_STRONG, 0x7A7A7A);
color!(AMBER, 0xFFC72C);
color!(BLACK_15, 0x000000, 0.15);
color!(BLACK_25, 0x000000, 0.25);
color!(BLACK_30, 0x000000, 0.3);
color!(BLACK_60, 0x000000, 0.6);
color!(BLACK_80, 0x000000, 0.80);

color!(MINUS_RED, 0xFF4C4C);
color!(BUSINESS_PRICE_ZEROES, 0x7E889F);
color!(BUSINESS_PRICE_ZEROES_BLINK, 0x7E889F, 0.8);
pub const BUSINESS_PRICE_SATS: iced::Color = BUSINESS_BLACK;
pub const BUSINESS_PRICE_SATS_BLINK: iced::Color = iced::Color {
    a: 0.95,
    ..BUSINESS_BLACK
};
color!(CARD_TEXT_SECONDARY, 0x738983);

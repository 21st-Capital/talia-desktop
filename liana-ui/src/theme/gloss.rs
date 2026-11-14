//! Talia: the iPhone app's glass finish, as far as iced can draw it.
//!
//! The app lays three layers over every tile, button and card: a brand-green
//! glow rising from the top edge, a sheen where the light catches the top-left,
//! and a pane edge. iced fills a widget with one colour or one linear gradient,
//! so the glow and the sheen become a single gradient from the top-left, and the
//! edge a single border colour. Every stop is a solid colour worked out from the
//! fill, so nothing behind the widget shows through.
use crate::color;
use iced::{gradient, Background, Color, Radians};

color!(GLOW, 0x124A38);
color!(LIGHT, 0xF4F1E8);
color!(SHADE, 0x01120F);
// The glow at the top of a page, already laid over the ground.
color!(PAGE_GLOW, 0x0E3B2E);

/// How much of the finish a card or a list row carries: the app's own number.
pub const CARD: f32 = 0.3;
/// Buttons and fields are smaller than cards and larger than the app's icon tiles.
pub const BUTTON: f32 = 0.6;

fn mix(a: Color, b: Color, t: f32) -> Color {
    Color {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: 1.0,
    }
}

fn luma(c: Color) -> f32 {
    0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b
}

/// A dark, opaque fill: the only kind that carries the glow and the light edge.
pub fn is_glass(base: Color) -> bool {
    base.a >= 1.0 && luma(base) < 0.3
}

/// The fill with the finish on it. See-through fills are left alone, a white
/// button only gets a faint shade, and a status colour gets the sheen but no glow.
pub fn fill(base: Color, strength: f32) -> Background {
    if base.a < 1.0 {
        return Background::Color(base);
    }
    let (start, middle, end) = if is_glass(base) {
        (
            mix(mix(base, GLOW, 0.5 * strength), LIGHT, 0.16 * strength),
            mix(mix(base, GLOW, 0.3 * strength), LIGHT, 0.03 * strength),
            mix(base, GLOW, 0.1 * strength),
        )
    } else if luma(base) > 0.8 {
        (base, base, mix(base, SHADE, 0.07))
    } else {
        (
            mix(base, LIGHT, 0.16 * strength),
            mix(base, LIGHT, 0.03 * strength),
            base,
        )
    };
    // From the top-left corner to the bottom-right one.
    gradient::Linear::new(Radians(std::f32::consts::PI * 0.75))
        .add_stop(0.0, start)
        .add_stop(0.5, middle)
        .add_stop(1.0, end)
        .into()
}

/// The light edge of a pane, between the app's bright top and dim bottom.
pub fn edge(base: Color) -> Color {
    mix(base, LIGHT, 0.18)
}

/// The soft brand glow at the top of a page, fading into the ground.
pub fn page(ground: Color) -> Background {
    gradient::Linear::new(Radians(std::f32::consts::PI))
        .add_stop(0.0, PAGE_GLOW)
        .add_stop(0.45, ground)
        .add_stop(1.0, ground)
        .into()
}

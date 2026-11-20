use super::{card::CARD_SHADOW, gloss, Theme};
use crate::color;
use iced::{
    border::Radius,
    widget::container::{transparent, Catalog, Style, StyleFn},
    Background, Border,
};

impl Catalog for Theme {
    type Class<'a> = StyleFn<'a, Self>;

    fn default<'a>() -> Self::Class<'a> {
        Box::new(transparent)
    }

    fn style(&self, class: &Self::Class<'_>) -> Style {
        class(self)
    }
}

/// Talia: the page glow, on the wallet theme only.
fn ground(theme: &Theme) -> Background {
    let ground = theme.colors.general.background;
    if theme.is_business() {
        return Background::Color(ground);
    }
    gloss::page(ground)
}

pub fn background(theme: &Theme) -> Style {
    Style {
        background: Some(ground(theme)),
        ..Default::default()
    }
}

pub fn panel_background(theme: &Theme) -> Style {
    Style {
        background: Some(ground(theme)),
        border: Border {
            color: color::TRANSPARENT,
            width: 1.0,
            radius: Radius {
                top_left: 24.0,
                top_right: 0.0,
                bottom_right: 0.0,
                bottom_left: 0.0,
            },
            ..Default::default()
        },
        ..Default::default()
    }
}

pub fn foreground(theme: &Theme) -> Style {
    Style {
        background: Some(Background::Color(theme.colors.general.foreground)),
        ..Default::default()
    }
}

pub fn top_bar(theme: &Theme) -> Style {
    Style {
        background: Some(Background::Color(theme.colors.general.menu_background)),
        ..Default::default()
    }
}

pub fn sidebar(theme: &Theme) -> Style {
    Style {
        background: Some(Background::Color(theme.colors.general.menu_background)),
        ..Default::default()
    }
}

pub fn border(theme: &Theme) -> Style {
    Style {
        background: Some(Background::Color(theme.colors.general.background)),
        ..Default::default()
    }
}

pub fn step_dot_filled(theme: &Theme) -> Style {
    Style {
        background: Some(Background::Color(theme.colors.general.accent)),
        border: Border {
            radius: 4.0.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

pub fn step_dot_track(_theme: &Theme) -> Style {
    Style {
        background: Some(Background::Color(color::BUSINESS_STEP_TRACK)),
        border: Border {
            radius: 4.0.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

pub fn tab_menu_panel(theme: &Theme) -> Style {
    let p = theme.colors.buttons.tab_menu.active;
    Style {
        background: Some(Background::Color(p.background)),
        border: Border {
            color: p.border.unwrap_or(p.background),
            width: 1.0,
            radius: Radius {
                top_left: 0.0,
                top_right: 0.0,
                bottom_right: 6.0,
                bottom_left: 6.0,
            },
            ..Default::default()
        },
        shadow: CARD_SHADOW,
        ..Default::default()
    }
}

pub fn form_field(theme: &Theme) -> Style {
    // Talia: the feerate box is drawn exactly like every other text field.
    if !theme.is_business() {
        let field = theme.colors.text_inputs.primary.active;
        return Style {
            background: Some(Background::Color(field.background)),
            border: Border {
                radius: 4.0.into(),
                width: 1.0,
                color: field.border.unwrap_or(color::TRANSPARENT),
                ..Default::default()
            },
            ..Default::default()
        };
    }
    let fill = theme.colors.general.form_field_background;
    let glass = !theme.is_business() && gloss::is_glass(fill);
    Style {
        background: Some(if glass {
            gloss::fill(fill, gloss::BUTTON)
        } else {
            Background::Color(fill)
        }),
        border: Border {
            radius: 4.0.into(),
            width: if glass { 1.0 } else { 0.0 },
            color: gloss::edge(fill),
            ..Default::default()
        },
        shadow: CARD_SHADOW,
        ..Default::default()
    }
}

pub fn custom(color: iced::Color) -> Box<dyn Fn(&Theme) -> Style> {
    Box::new(move |_theme: &Theme| Style {
        background: Some(Background::Color(color)),
        ..Default::default()
    })
}

use iced::widget::canvas::LineDash;
use iced::Color;

use crate::color::TRANSPARENT;

use super::{
    liana_business::{EXTERNAL, SAFETY_NET},
    *,
};

color!(BTN_PRIMARY_HOVER_BGD, 0xE5E9E8);
color!(BTN_PRIMARY_PRESSED_BGD, 0xC9D1CE);
color!(BTN_PRIMARY_DISABLED_BGD, 0x12221E);
color!(BTN_PRIMARY_TEXT, 0x01120F);
color!(BTN_PRIMARY_PRESSED_TEXT, 0x01120F);
color!(BTN_PRIMARY_DISABLED_TEXT, 0x556762);
color!(BTN_PRIMARY_DISABLED_BORDER, 0x23312D);

color!(BTN_SECONDARY_HOVER_BGD, 0x314640);
color!(BTN_SECONDARY_PRESSED_BGD, 0x173029);
color!(BTN_SECONDARY_TEXT, 0x16C784);
color!(BTN_SECONDARY_HOVER_TEXT, 0x4FD9A3);
color!(BTN_SECONDARY_PRESSED_TEXT, 0x8BE8C4);

// The timeline diagram's tints: one per key kind, and one per rail state.
color!(TIMELINE_RECOVERY_FILL, 0x2E2410);
color!(TIMELINE_RECOVERY_RIM, 0x4D3D14);
color!(TIMELINE_NEUTRAL_FILL, 0x1E2D29);
color!(TIMELINE_LOCKED_INK, 0xA9B5B1);
color!(TIMELINE_RAIL_LOCKED, 0x556762);

color!(BTN_TERTIARY_BGD, 0x233A34);
color!(BTN_TERTIARY_HOVER_BGD, 0x314640);
color!(BTN_TERTIARY_PRESSED_BGD, 0x173029);
color!(BTN_TERTIARY_TEXT, 0xE5E9E8);
const BTN_TERTIARY_HOVER_TEXT: Color = color::WHITE;
color!(BTN_TERTIARY_PRESSED_TEXT, 0xA9B5B1);

// Talia: the iPhone app's tokens (keel-wallet src/ui/theme/colors.ts).
// A button that cannot be pressed is a muted colour, never an opacity.
color!(BTN_ACTION_DISABLED_BGD, 0x737D7B);
color!(BTN_ACTION_DISABLED_TEXT, 0x404D4A);
color!(BTN_SECONDARY_BGD, 0x233A34);
color!(BTN_SECONDARY_INK, 0xF8F9F9);
color!(BTN_SECONDARY_RIM, 0x3F534B);
// Cards, rows and fields: the app's glass (off-white over the glowing ground) as solid
// colours, because a see-through card shows whatever Liana draws behind it.
color!(CARD_BGD, 0x173029);
color!(CARD_EDGE, 0x273E38);
// Text fields (option B2, picked 2026-11-19): a solid box clearly lighter than the cards
// and the page, with a light edge, so every field shows wherever it sits.
color!(FIELD_BGD, 0x35554B);
color!(FIELD_EDGE, 0x5A776C);
const FIELD_PLACEHOLDER: Color = TIMELINE_LOCKED_INK;
const FIELD_TEXT: Color = BTN_SECONDARY_INK;
// The sidebar and the top bar, and the sidebar's hovered and selected item.
color!(MENU_BGD, 0x01120F);
color!(MENU_HOVER_BGD, 0x12221E);
color!(MENU_ACTIVE_BGD, 0x042F22);
color!(MENU_ACTIVE_TEXT, 0x16C784);

fn btn_action_disabled() -> Option<ButtonPalette> {
    Some(ButtonPalette {
        background: BTN_ACTION_DISABLED_BGD,
        text: BTN_ACTION_DISABLED_TEXT,
        border: BTN_ACTION_DISABLED_BGD.into(),
        shadow: Default::default(),
    })
}

fn btn_disabled() -> Option<ButtonPalette> {
    Some(ButtonPalette {
        background: BTN_PRIMARY_DISABLED_BGD,
        text: BTN_PRIMARY_DISABLED_TEXT,
        border: BTN_PRIMARY_DISABLED_BORDER.into(),
        shadow: Default::default(),
    })
}

impl Palette {
    pub fn liana() -> Self {
        Self {
            general: General {
                background: color::LIGHT_BLACK,
                menu_background: MENU_BGD,
                foreground: MENU_BGD,
                scrollable: color::GREY_7,
                accent: color::GREEN,
                form_field_background: FIELD_BGD,
            },
            text: Text {
                primary: color::WHITE,
                secondary: color::GREY_2,
                muted: color::GREY_3,
                border: color::GREY_7,
                warning: color::ORANGE,
                success: color::GREEN,
                error: color::RED,
                accent: color::GREEN,
                card_secondary: color::CARD_TEXT_SECONDARY,
                address: color::GREY_2,
                address_dimmed: color::ORANGE,
            },
            price: Price {
                zeroes: color::GREY_3,
                sats: color::WHITE,
                blink_zeroes: color::GREY_4,
                blink_sats: color::GREY_2,
                receive: color::SUCCESS_GREEN,
                send: color::MINUS_RED,
                refresh: color::BLUE,
            },
            buttons: Buttons {
                border_width: 1.0,
                primary: Button {
                    active: ButtonPalette {
                        background: color::WHITE,
                        text: BTN_PRIMARY_TEXT,
                        border: TRANSPARENT.into(),
                        shadow: Default::default(),
                    },
                    hovered: ButtonPalette {
                        background: BTN_PRIMARY_HOVER_BGD,
                        text: BTN_PRIMARY_TEXT,
                        border: TRANSPARENT.into(),
                        shadow: Default::default(),
                    },
                    pressed: Some(ButtonPalette {
                        background: BTN_PRIMARY_PRESSED_BGD,
                        text: BTN_PRIMARY_PRESSED_TEXT,
                        border: TRANSPARENT.into(),
                        shadow: Default::default(),
                    }),
                    disabled: btn_action_disabled(),
                },
                secondary: Button {
                    active: ButtonPalette {
                        background: BTN_SECONDARY_BGD,
                        text: BTN_SECONDARY_INK,
                        border: BTN_SECONDARY_RIM.into(),
                        shadow: Default::default(),
                    },
                    hovered: ButtonPalette {
                        background: BTN_SECONDARY_HOVER_BGD,
                        text: BTN_SECONDARY_INK,
                        border: color::GREEN.into(),
                        shadow: Default::default(),
                    },
                    pressed: Some(ButtonPalette {
                        background: BTN_SECONDARY_PRESSED_BGD,
                        text: BTN_SECONDARY_INK,
                        border: color::GREEN.into(),
                        shadow: Default::default(),
                    }),
                    disabled: btn_action_disabled(),
                },
                auxiliary: Button {
                    active: ButtonPalette {
                        background: color::TRANSPARENT,
                        text: BTN_SECONDARY_TEXT,
                        border: color::GREY_2.into(),
                        shadow: Default::default(),
                    },
                    hovered: ButtonPalette {
                        background: color::TRANSPARENT,
                        text: BTN_SECONDARY_HOVER_TEXT,
                        border: color::GREEN.into(),
                        shadow: Default::default(),
                    },
                    pressed: Some(ButtonPalette {
                        background: color::TRANSPARENT,
                        text: BTN_SECONDARY_PRESSED_TEXT,
                        border: color::GREEN.into(),
                        shadow: Default::default(),
                    }),
                    disabled: btn_disabled(),
                },
                feerate: feerate_button(color::GREEN, BTN_PRIMARY_TEXT),
                feerate_unselected: feerate_unselected_button(
                    color::GREY_5,
                    color::WHITE,
                    color::GREEN,
                ),
                tertiary: Button {
                    active: ButtonPalette {
                        background: BTN_TERTIARY_BGD,
                        text: BTN_TERTIARY_TEXT,
                        border: TRANSPARENT.into(),
                        shadow: Default::default(),
                    },
                    hovered: ButtonPalette {
                        background: BTN_TERTIARY_HOVER_BGD,
                        text: BTN_TERTIARY_HOVER_TEXT,
                        border: TRANSPARENT.into(),
                        shadow: Default::default(),
                    },
                    pressed: Some(ButtonPalette {
                        background: BTN_TERTIARY_PRESSED_BGD,
                        text: BTN_TERTIARY_PRESSED_TEXT,
                        border: TRANSPARENT.into(),
                        shadow: Default::default(),
                    }),
                    disabled: btn_action_disabled(),
                },
                destructive: Button {
                    active: ButtonPalette {
                        background: color::GREY_6,
                        text: color::RED,
                        border: color::RED.into(),
                        shadow: Default::default(),
                    },
                    hovered: ButtonPalette {
                        background: color::RED,
                        text: color::LIGHT_BLACK,
                        border: color::RED.into(),
                        shadow: Default::default(),
                    },
                    pressed: Some(ButtonPalette {
                        background: color::RED,
                        text: color::LIGHT_BLACK,
                        border: color::RED.into(),
                        shadow: Default::default(),
                    }),
                    disabled: btn_action_disabled(),
                },
                transparent: Button {
                    active: ButtonPalette {
                        background: color::TRANSPARENT,
                        text: color::GREY_2,
                        border: None,
                        shadow: Default::default(),
                    },
                    hovered: ButtonPalette {
                        background: color::TRANSPARENT,
                        text: color::GREY_2,
                        border: None,
                        shadow: Default::default(),
                    },
                    pressed: Some(ButtonPalette {
                        background: color::TRANSPARENT,
                        text: color::GREY_2,
                        border: None,
                        shadow: Default::default(),
                    }),
                    disabled: Some(ButtonPalette {
                        background: color::TRANSPARENT,
                        text: Color {
                            a: 0.5,
                            ..color::GREY_2
                        },
                        border: None,
                        shadow: Default::default(),
                    }),
                },
                remove: Button {
                    active: ButtonPalette {
                        background: color::TRANSPARENT,
                        text: color::GREY_2,
                        border: None,
                        shadow: Default::default(),
                    },
                    hovered: ButtonPalette {
                        background: color::TRANSPARENT,
                        text: color::GREY_2,
                        border: None,
                        shadow: Default::default(),
                    },
                    pressed: Some(ButtonPalette {
                        background: color::TRANSPARENT,
                        text: color::GREY_2,
                        border: None,
                        shadow: Default::default(),
                    }),
                    disabled: Some(ButtonPalette {
                        background: color::TRANSPARENT,
                        text: color::GREY_2,
                        border: None,
                        shadow: Default::default(),
                    }),
                },
                transparent_border: Button {
                    active: ButtonPalette {
                        background: color::TRANSPARENT,
                        text: color::GREY_2,
                        border: color::TRANSPARENT.into(),
                        shadow: Default::default(),
                    },
                    hovered: ButtonPalette {
                        background: color::TRANSPARENT,
                        text: color::GREY_2,
                        border: color::GREEN.into(),
                        shadow: Default::default(),
                    },
                    pressed: Some(ButtonPalette {
                        background: color::TRANSPARENT,
                        text: color::GREY_2,
                        border: color::GREEN.into(),
                        shadow: Default::default(),
                    }),
                    disabled: btn_disabled(),
                },
                list_entry: Button {
                    active: ButtonPalette {
                        background: CARD_BGD,
                        text: color::GREY_2,
                        border: CARD_EDGE.into(),
                        shadow: Default::default(),
                    },
                    hovered: ButtonPalette {
                        background: CARD_BGD,
                        text: color::GREEN,
                        border: color::GREEN.into(),
                        shadow: Default::default(),
                    },
                    pressed: Some(ButtonPalette {
                        background: BTN_SECONDARY_HOVER_BGD,
                        text: color::GREEN,
                        border: color::GREEN.into(),
                        shadow: Default::default(),
                    }),
                    disabled: btn_disabled(),
                },
                list_entry_radius: None,
                list_entry_hover_border_width: None,
                container: Button {
                    active: ButtonPalette {
                        background: color::TRANSPARENT,
                        text: color::GREY_2,
                        border: None,
                        shadow: Default::default(),
                    },
                    hovered: ButtonPalette {
                        background: color::TRANSPARENT,
                        text: color::GREY_2,
                        border: None,
                        shadow: Default::default(),
                    },
                    pressed: Some(ButtonPalette {
                        background: color::TRANSPARENT,
                        text: color::GREY_2,
                        border: None,
                        shadow: Default::default(),
                    }),
                    disabled: btn_disabled(),
                },
                container_border: Button {
                    active: ButtonPalette {
                        background: color::TRANSPARENT,
                        text: color::GREY_2,
                        border: color::TRANSPARENT.into(),
                        shadow: Default::default(),
                    },
                    hovered: ButtonPalette {
                        background: color::TRANSPARENT,
                        text: color::GREY_2,
                        border: color::GREEN.into(),
                        shadow: Default::default(),
                    },
                    pressed: Some(ButtonPalette {
                        background: color::TRANSPARENT,
                        text: color::GREY_2,
                        border: color::GREEN.into(),
                        shadow: Default::default(),
                    }),
                    disabled: btn_disabled(),
                },
                menu: Button {
                    active: ButtonPalette {
                        background: color::TRANSPARENT,
                        text: color::WHITE,
                        border: color::TRANSPARENT.into(),
                        shadow: Default::default(),
                    },
                    hovered: ButtonPalette {
                        background: MENU_HOVER_BGD,
                        text: color::WHITE,
                        border: color::TRANSPARENT.into(),
                        shadow: Default::default(),
                    },
                    pressed: Some(ButtonPalette {
                        background: MENU_ACTIVE_BGD,
                        text: MENU_ACTIVE_TEXT,
                        border: color::TRANSPARENT.into(),
                        shadow: Default::default(),
                    }),
                    disabled: Some(ButtonPalette {
                        background: color::TRANSPARENT,
                        text: Color {
                            a: 0.5,
                            ..color::WHITE
                        },
                        border: color::TRANSPARENT.into(),
                        shadow: Default::default(),
                    }),
                },
                tab_menu: Button {
                    active: ButtonPalette {
                        background: color::GREY_6,
                        text: color::GREY_2,
                        border: color::GREY_7.into(),
                        shadow: Default::default(),
                    },
                    hovered: ButtonPalette {
                        background: color::GREY_6,
                        text: color::GREEN,
                        border: color::GREEN.into(),
                        shadow: Default::default(),
                    },
                    pressed: Some(ButtonPalette {
                        background: color::GREY_6,
                        text: color::GREEN,
                        border: color::GREEN.into(),
                        shadow: Default::default(),
                    }),
                    disabled: Some(ButtonPalette {
                        background: color::GREY_6,
                        text: Color {
                            a: 0.5,
                            ..color::GREY_2
                        },
                        border: color::GREY_7.into(),
                        shadow: Default::default(),
                    }),
                },
                tab: Button {
                    active: ButtonPalette {
                        background: color::GREY_6,
                        text: color::GREY_2,
                        border: color::GREY_7.into(),
                        shadow: Default::default(),
                    },
                    hovered: ButtonPalette {
                        background: color::GREY_6,
                        text: color::GREEN,
                        border: color::GREEN.into(),
                        shadow: Default::default(),
                    },
                    pressed: Some(ButtonPalette {
                        background: color::LIGHT_BLACK,
                        text: color::GREEN,
                        border: color::GREEN.into(),
                        shadow: Default::default(),
                    }),
                    disabled: Some(ButtonPalette {
                        background: color::GREY_6,
                        text: Color {
                            a: 0.5,
                            ..color::GREY_2
                        },
                        border: color::GREY_7.into(),
                        shadow: Default::default(),
                    }),
                },
                link: Button {
                    active: ButtonPalette {
                        background: color::TRANSPARENT,
                        text: color::GREY_2,
                        border: color::TRANSPARENT.into(),
                        shadow: Default::default(),
                    },
                    hovered: ButtonPalette {
                        background: color::TRANSPARENT,
                        text: color::GREEN,
                        border: color::TRANSPARENT.into(),
                        shadow: Default::default(),
                    },
                    pressed: Some(ButtonPalette {
                        background: color::TRANSPARENT,
                        text: color::GREEN,
                        border: color::TRANSPARENT.into(),
                        shadow: Default::default(),
                    }),
                    disabled: Some(ButtonPalette {
                        background: color::TRANSPARENT,
                        text: Color {
                            a: 0.5,
                            ..color::GREY_2
                        },
                        border: color::TRANSPARENT.into(),
                        shadow: Default::default(),
                    }),
                },
                link_subtle: Button {
                    active: ButtonPalette {
                        background: color::TRANSPARENT,
                        text: color::GREY_3,
                        border: color::TRANSPARENT.into(),
                        shadow: Default::default(),
                    },
                    hovered: ButtonPalette {
                        background: color::TRANSPARENT,
                        text: color::GREY_2,
                        border: color::TRANSPARENT.into(),
                        shadow: Default::default(),
                    },
                    pressed: Some(ButtonPalette {
                        background: color::TRANSPARENT,
                        text: color::GREY_2,
                        border: color::TRANSPARENT.into(),
                        shadow: Default::default(),
                    }),
                    disabled: None,
                },
                pick_list: Button {
                    active: ButtonPalette {
                        background: color::GREY_6,
                        text: color::GREY_2,
                        border: color::GREY_7.into(),
                        shadow: Default::default(),
                    },
                    hovered: ButtonPalette {
                        background: color::GREY_6,
                        text: color::GREEN,
                        border: color::GREEN.into(),
                        shadow: Default::default(),
                    },
                    pressed: Some(ButtonPalette {
                        background: color::GREY_6,
                        text: color::GREEN,
                        border: color::GREEN.into(),
                        shadow: Default::default(),
                    }),
                    disabled: btn_disabled(),
                },
                signing_devices: Button {
                    active: ButtonPalette {
                        background: BTN_SECONDARY_BGD,
                        text: BTN_SECONDARY_INK,
                        border: BTN_SECONDARY_RIM.into(),
                        shadow: Default::default(),
                    },
                    hovered: ButtonPalette {
                        background: BTN_SECONDARY_HOVER_BGD,
                        text: BTN_SECONDARY_INK,
                        border: color::GREEN.into(),
                        shadow: Default::default(),
                    },
                    pressed: Some(ButtonPalette {
                        background: BTN_SECONDARY_PRESSED_BGD,
                        text: BTN_SECONDARY_INK,
                        border: color::GREEN.into(),
                        shadow: Default::default(),
                    }),
                    disabled: btn_disabled(),
                },
                optional_section: Button {
                    active: ButtonPalette {
                        background: color::TRANSPARENT,
                        text: color::GREY_2,
                        border: color::TRANSPARENT.into(),
                        shadow: Default::default(),
                    },
                    hovered: ButtonPalette {
                        background: color::TRANSPARENT,
                        text: color::GREY_2,
                        border: color::GREEN.into(),
                        shadow: Default::default(),
                    },
                    pressed: Some(ButtonPalette {
                        background: color::TRANSPARENT,
                        text: color::GREY_2,
                        border: color::GREEN.into(),
                        shadow: Default::default(),
                    }),
                    disabled: btn_disabled(),
                },
            },
            cards: Cards {
                simple: ContainerPalette {
                    background: CARD_BGD,
                    text: None,
                    border: Some(CARD_EDGE),
                },
                transparent: ContainerPalette {
                    background: color::TRANSPARENT,
                    text: None,
                    border: Some(color::TRANSPARENT),
                },
                modal: ContainerPalette {
                    background: color::LIGHT_BLACK,
                    text: None,
                    border: CARD_EDGE.into(),
                },
                border: ContainerPalette {
                    background: color::TRANSPARENT,
                    text: None,
                    border: color::GREY_7.into(),
                },
                invalid: ContainerPalette {
                    background: color::LIGHT_BLACK,
                    text: color::RED.into(),
                    border: color::RED.into(),
                },
                legacy_warning: ContainerPalette {
                    background: color::LIGHT_BLACK,
                    text: color::ORANGE.into(),
                    border: color::ORANGE.into(),
                },
                warning: ContainerPalette {
                    background: color::LIGHT_BLACK,
                    text: color::ORANGE.into(),
                    border: color::ORANGE.into(),
                },
                soft_warning: ContainerPalette {
                    background: color::TRANSPARENT,
                    text: color::ORANGE.into(),
                    border: color::GREY_7.into(),
                },
                info: ContainerPalette {
                    background: color::TRANSPARENT,
                    text: color::WHITE.into(),
                    border: color::GREY_7.into(),
                },
                success: None,
                error: ContainerPalette {
                    background: color::LIGHT_BLACK,
                    text: color::RED.into(),
                    border: color::RED.into(),
                },
                section: ContainerPalette {
                    background: color::GREY_4,
                    text: None,
                    border: Some(color::TRANSPARENT),
                },
                flat: ContainerPalette {
                    background: CARD_BGD,
                    text: None,
                    border: Some(color::TRANSPARENT),
                },
            },
            banners: Banners {
                network: ContainerPalette {
                    background: color::AMBER,
                    text: color::LIGHT_BLACK.into(),
                    border: None,
                },
                warning: ContainerPalette {
                    background: color::ORANGE,
                    text: color::LIGHT_BLACK.into(),
                    border: None,
                },
            },
            badges: Badges {
                simple: ContainerPalette {
                    background: color::GREY_4,
                    text: None,
                    border: color::TRANSPARENT.into(),
                },
                bitcoin: ContainerPalette {
                    background: color::ORANGE,
                    text: color::WHITE.into(),
                    border: color::TRANSPARENT.into(),
                },
                success: ContainerPalette {
                    background: color::GREEN,
                    text: color::BLACK.into(),
                    border: color::TRANSPARENT.into(),
                },
                avatar: ContainerPalette {
                    background: color::FINGERPRINT_BACKGROUND,
                    text: color::GREY_2.into(),
                    border: color::TRANSPARENT.into(),
                },
                danger: None,
            },
            tile_tones: Tiles {
                background: color::GREY_5,
                accent: Tile {
                    fg: color::GREEN,
                    bg: None,
                },
                neutral: Tile {
                    fg: SAFETY_NET,
                    bg: None,
                },
                muted: Tile {
                    fg: color::GREY_3,
                    bg: None,
                },
                danger: Tile {
                    fg: color::RED,
                    bg: None,
                },
            },
            pills: Pills {
                simple: ContainerPalette {
                    background: color::TRANSPARENT,
                    text: color::GREY_3.into(),
                    border: color::GREY_3.into(),
                },
                simple_fill: ContainerPalette {
                    background: color::GREY_3,
                    text: color::LIGHT_BLACK.into(),
                    border: color::GREY_3.into(),
                },
                warning: ContainerPalette {
                    background: color::AMBER,
                    text: color::DARK_TEXT_SECONDARY.into(),
                    border: color::AMBER.into(),
                },
                soft_warning: ContainerPalette {
                    background: color::TRANSPARENT,
                    text: color::AMBER.into(),
                    border: color::AMBER.into(),
                },
                role_manager: ContainerPalette {
                    background: color::GREY_5,
                    text: color::GREY_2.into(),
                    border: color::GREY_5.into(),
                },
                role_participant: ContainerPalette {
                    background: color::GREY_5,
                    text: color::GREY_2.into(),
                    border: color::GREY_5.into(),
                },
                success: ContainerPalette {
                    background: color::SUCCESS_GREEN,
                    text: color::LIGHT_BLACK.into(),
                    border: color::SUCCESS_GREEN.into(),
                },
                internal: ContainerPalette {
                    background: color::BLUE,
                    text: color::LIGHT_BLACK.into(),
                    border: color::BLUE.into(),
                },
                external: ContainerPalette {
                    background: EXTERNAL,
                    text: color::WHITE.into(),
                    border: EXTERNAL.into(),
                },
                safety_net: ContainerPalette {
                    background: SAFETY_NET,
                    text: color::WHITE.into(),
                    border: SAFETY_NET.into(),
                },
                fingerprint: ContainerPalette {
                    background: color::FINGERPRINT_BACKGROUND,
                    text: color::FINGERPRINT_TEXT.into(),
                    border: color::FINGERPRINT_BORDER.into(),
                },
            },
            tabs: Tabs {
                active: color::GREEN,
                inactive: color::GREY_2,
                strip: color::GREY_7,
                dot_ready: color::GREEN,
                dot_pending: color::AMBER,
            },
            notifications: Notifications {
                pending: ContainerPalette {
                    background: color::GREEN,
                    text: color::LIGHT_BLACK.into(),
                    border: Some(color::GREEN),
                },
                error: ContainerPalette {
                    background: color::ORANGE,
                    text: color::LIGHT_BLACK.into(),
                    border: Some(color::ORANGE),
                },
            },
            text_inputs: TextInputs {
                primary: TextInput {
                    active: TextInputPalette {
                        background: FIELD_BGD,
                        icon: color::TRANSPARENT,
                        placeholder: FIELD_PLACEHOLDER,
                        value: FIELD_TEXT,
                        selection: color::TRANSPARENT_GREEN,
                        border: Some(FIELD_EDGE),
                    },
                    disabled: TextInputPalette {
                        background: FIELD_BGD,
                        icon: color::TRANSPARENT,
                        placeholder: FIELD_PLACEHOLDER,
                        value: FIELD_TEXT,
                        selection: color::TRANSPARENT_GREEN,
                        border: Some(FIELD_EDGE),
                    },
                },
                invalid: TextInput {
                    active: TextInputPalette {
                        background: FIELD_BGD,
                        icon: color::TRANSPARENT,
                        placeholder: FIELD_PLACEHOLDER,
                        value: FIELD_TEXT,
                        selection: color::TRANSPARENT_GREEN,
                        border: Some(color::RED),
                    },
                    disabled: TextInputPalette {
                        background: FIELD_BGD,
                        icon: color::TRANSPARENT,
                        placeholder: FIELD_PLACEHOLDER,
                        value: color::TRANSPARENT,
                        selection: color::TRANSPARENT_GREEN,
                        border: Some(color::RED),
                    },
                },
            },
            combobox: Combobox {
                selected: color::FINGERPRINT_BACKGROUND,
                header: color::GREY_7,
            },
            checkboxes: Checkboxes {
                icon: color::GREEN,
                text: color::GREY_2,
                background: color::TRANSPARENT,
                border: Some(color::CHECKBOX_BORDER),
            },
            radio_buttons: RadioButtons {
                dot: color::GREEN,
                text: color::GREY_2,
                border: color::GREY_7,
            },
            sliders: Sliders {
                background: color::GREEN,
                border: color::GREEN,
                rail_border: None,
                rail_backgrounds: (color::GREEN, color::GREY_7),
            },
            progress_bars: ProgressBars {
                bar: color::GREEN,
                border: color::TRANSPARENT.into(),
                background: color::GREY_6,
            },
            rule: color::GREY_7,
            pane_grid: PaneGrid {
                background: MENU_BGD,
                highlight_border: color::GREEN,
                highlight_background: color::TRANSPARENT_GREEN,
                picked_split: color::GREEN,
                hovered_split: color::GREEN,
            },
            togglers: Togglers {
                on: Toggler {
                    background: color::GREEN,
                    background_border: color::GREEN,
                    foreground: color::WHITE,
                    foreground_border: color::WHITE,
                },
                off: Toggler {
                    background: color::GREY_4,
                    background_border: color::GREY_4,
                    foreground: color::WHITE,
                    foreground_border: color::WHITE,
                },
            },
            menus: Menus {
                pick_list: Menu {
                    border: color::GREY_7,
                    text: color::GREY_2,
                    selected_text: color::BLACK,
                    background: color::GREY_6,
                    selected_background: color::GREEN,
                },
            },
            spinner: Spinner {
                track: color::GREY_5,
                arc: color::GREEN,
            },
            timeline: Timeline {
                primary: TimelineTone {
                    ink: color::GREEN,
                    fill: color::FINGERPRINT_BACKGROUND,
                    rim: color::FINGERPRINT_BORDER,
                },
                recovery: TimelineTone {
                    ink: color::ORANGE,
                    fill: TIMELINE_RECOVERY_FILL,
                    rim: TIMELINE_RECOVERY_RIM,
                },
                inheritance: TimelineTone {
                    ink: color::GREY_1,
                    fill: TIMELINE_NEUTRAL_FILL,
                    rim: color::GREY_7,
                },
                spendable: TimelineTone {
                    ink: color::GREEN,
                    fill: color::FINGERPRINT_BACKGROUND,
                    rim: color::FINGERPRINT_BORDER,
                },
                locked: TimelineTone {
                    ink: TIMELINE_LOCKED_INK,
                    fill: TIMELINE_NEUTRAL_FILL,
                    rim: color::GREY_7,
                },
                rail_spend: TimelineRail {
                    color: color::GREEN,
                    dash: None,
                },
                rail_locked: TimelineRail {
                    color: TIMELINE_RAIL_LOCKED,
                    dash: Some(LineDash {
                        segments: &[3.0, 6.0],
                        offset: 0,
                    }),
                },
                badge_ink: color::LIGHT_BLACK,
            },
        }
    }
}

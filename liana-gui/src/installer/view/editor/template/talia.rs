//! The iPhone app's vault shapes: their introductions and their key editor.

use std::fmt;

use iced::{
    widget::{column, row, Space},
    Alignment, Length,
};
use liana::miniscript::bitcoin::Network;

use liana_ui::{
    color,
    component::{
        button::{self, btn_remove},
        card,
        checkbox::labelled_radio,
        pick_list::pick_list,
        text::{new, p1_bold, p2_regular, H3_SIZE},
        timeline::{spending_timeline, Template},
        tooltip::tooltip,
    },
    icon,
    spacing::{HSpacing, VSpacing},
    theme,
    widget::*,
};

use crate::installer::{
    descriptor::{Key, Path},
    message::{self, Message, TaliaMessage},
    step::descriptor::editor::talia::{
        KeysFlow, TaliaShape, TaliaTemplate, HEIR_MONTHS, MAX_HEIRS, RESCUE_MONTHS,
    },
    view::{
        defined_threshold,
        editor::{
            defined_key, descriptor_type,
            template::{
                caption_block, key_legend, row_next, BOTTOM_PADDING, DESCRIPTION_BOTTOM_PADDING,
                FOOTER_SPACING, KEY_LEGEND_SPACING,
            },
            undefined_key,
        },
        layout,
    },
};
use crate::t;

/// The client's keys.
const OWN: iced::Color = color::GREEN;
/// The heirs' keys.
const HEIR: iced::Color = color::WHITE;
/// 21st Capital's keys.
const OURS: iced::Color = color::ORANGE;

fn title(template: TaliaTemplate) -> String {
    match template {
        TaliaTemplate::TwoOfThree => t!("installer-talia-multi-2-3"),
        TaliaTemplate::TwoOfTwo => t!("installer-talia-multi-2-2"),
        TaliaTemplate::SingleWithRescue => t!("installer-talia-single"),
    }
}

fn timeline(template: TaliaTemplate) -> Template {
    match template {
        TaliaTemplate::TwoOfThree => Template::TwoOfThree,
        TaliaTemplate::TwoOfTwo => Template::TwoOfTwo,
        TaliaTemplate::SingleWithRescue => Template::SingleWithRescue,
    }
}

pub fn talia_template_description(
    template: TaliaTemplate,
    progress: (usize, usize),
    network: Network,
) -> Element<'static, Message> {
    let intro = caption_block(match template {
        TaliaTemplate::TwoOfThree => t!("installer-talia-multi-2-3-description"),
        TaliaTemplate::TwoOfTwo => t!("installer-talia-multi-2-2-description"),
        TaliaTemplate::SingleWithRescue => t!("installer-talia-single-description"),
    });

    let keys = row![
        key_legend(theme::text::success, t!("installer-talia-legend-your-keys")),
        key_legend(theme::text::primary, t!("installer-talia-legend-heir-keys")),
        key_legend(theme::text::warning, t!("installer-talia-legend-21c-keys")),
    ]
    .spacing(KEY_LEGEND_SPACING);

    let explanation = caption_block(match template {
        TaliaTemplate::TwoOfThree => t!("installer-talia-explain-multi"),
        TaliaTemplate::TwoOfTwo => t!("installer-talia-explain-2-2"),
        TaliaTemplate::SingleWithRescue => t!("installer-talia-explain-single"),
    });

    let content = column![
        new::b1_bold(title(template)),
        intro,
        keys,
        explanation,
        spending_timeline(timeline(template)),
        row_next(),
        Space::with_height(DESCRIPTION_BOTTOM_PADDING),
    ]
    .align_x(Alignment::Start)
    .spacing(VSpacing::L);

    layout(
        progress,
        network,
        None,
        t!("installer-introduction"),
        content,
        Some(Message::Previous),
    )
}

/// What the key editor of a shape draws from.
pub struct TaliaEditor<'a> {
    pub shape: &'a TaliaShape,
    pub paths: &'a [Path],
    pub flow: &'a KeysFlow,
    /// The website was asked for this shape's keys: it no longer changes.
    pub locked: bool,
    /// 21st Capital issues keys on the wallet's network.
    pub keys_available: bool,
    /// Taproot or SegWit v0, the client's choice as in Liana's own templates.
    pub use_taproot: bool,
    pub editing_descriptor_type: bool,
    pub valid: bool,
    pub processing: bool,
}

fn talia(msg: TaliaMessage) -> Message {
    Message::DefineDescriptor(message::DefineDescriptor::Talia(msg))
}

/// A month count in the heir's wait picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Months(u8);

impl fmt::Display for Months {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", t!("duration-months", count = self.0))
    }
}

/// One of the client's or an heir's key slots, edited as Liana edits keys.
fn key_slot<'a>(
    key: Option<&'a Key>,
    color: iced::Color,
    label: String,
    path: usize,
    index: usize,
    use_taproot: bool,
) -> Element<'a, Message> {
    match key {
        Some(key) => defined_key(
            &key.name,
            color,
            label,
            use_taproot.then(|| key.source.taproot_warning()).flatten(),
            use_taproot.then(|| key.source.taproot_note()).flatten(),
            true,
        ),
        None => undefined_key(color, label, true, true),
    }
    .map(move |msg| {
        Message::DefineDescriptor(message::DefineDescriptor::Path(
            path,
            message::DefinePath::Key(index, msg),
        ))
    })
}

/// One of 21st Capital's slots: filled by "Get my 21st Capital keys", never
/// by hand.
fn our_slot<'a>(key: Option<&'a Key>, label: String) -> Element<'a, Message> {
    if let Some(key) = key {
        // Nothing on it to press: the key came with its slot. Its name is
        // already the slot's label ("21st Capital Rescue Key"), so it is
        // written once, not as label and name side by side.
        return card::simple(
            row![
                icon::round_key_icon().size(H3_SIZE).color(OURS),
                p1_bold(&key.name).width(Length::Fill),
                icon::check_icon().style(theme::text::success),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        )
        .into();
    }
    card::simple(
        row![
            icon::round_key_icon().size(H3_SIZE).color(OURS),
            column![
                p2_regular(label).style(theme::text::secondary),
                new::caption(t!("installer-talia-slot-pending")).style(theme::text::secondary),
            ]
            .spacing(5)
            .width(Length::Fill),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    )
    .into()
}

/// A section's heading. Its explanation sits behind the info dot, never as a
/// paragraph under it: the screen shows headings and controls only.
fn heading<'a>(title: String, help: String) -> Element<'a, Message> {
    row![p1_bold(title), tooltip(help)]
        .spacing(6)
        .align_y(Alignment::Center)
        .into()
}

fn section<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    Container::new(content)
        .padding(10)
        .width(Length::Fill)
        .style(theme::card::border)
        .into()
}

fn primary_path<'a>(e: &TaliaEditor<'a>) -> Element<'a, Message> {
    let template = e.shape.template;
    let primary = &e.paths[0];
    let own = template.own_keys();
    let mut keys = column![].spacing(5);
    for (j, key) in primary.keys.iter().enumerate() {
        keys = keys.push(if j < own {
            let label = if own > 1 {
                t!("installer-talia-your-key-number", number = j + 1)
            } else {
                t!("installer-talia-your-key")
            };
            key_slot(key.as_ref(), OWN, label, 0, j, e.use_taproot)
        } else if e.shape.cosigner_21c {
            our_slot(key.as_ref(), t!("installer-talia-cosigner"))
        } else {
            key_slot(
                key.as_ref(),
                OWN,
                t!("installer-talia-your-extra-key"),
                0,
                j,
                e.use_taproot,
            )
        });
    }

    let holder: Option<Element<'a, Message>> = template.has_cosigner_slot().then(|| {
        let question = if own == 2 {
            t!("installer-talia-who-holds-third")
        } else {
            t!("installer-talia-who-holds-second")
        };
        if e.locked {
            new::caption(format!(
                "{question} {}",
                if e.shape.cosigner_21c {
                    t!("installer-talia-holder-21c")
                } else {
                    t!("installer-talia-holder-own")
                }
            ))
            .style(theme::text::secondary)
            .into()
        } else {
            row![
                new::caption(question).style(theme::text::secondary),
                labelled_radio(
                    t!("installer-talia-holder-21c"),
                    e.shape.cosigner_21c,
                    talia(TaliaMessage::CosignerOurs(true)),
                ),
                labelled_radio(
                    t!("installer-talia-holder-own"),
                    !e.shape.cosigner_21c,
                    talia(TaliaMessage::CosignerOurs(false)),
                ),
            ]
            .spacing(HSpacing::L)
            .align_y(Alignment::Center)
            .into()
        }
    });

    let threshold = (primary.keys.len() > 1).then(|| {
        Element::from(defined_threshold(
            OWN,
            true,
            (primary.threshold, primary.keys.len()),
        ))
        .map(|msg| Message::DefineDescriptor(message::DefineDescriptor::Path(0, msg)))
    });

    section(
        column![heading(
            t!("installer-talia-primary-keys"),
            t!("installer-able-to-move-any-time"),
        )]
        .push_maybe(holder)
        .push(keys)
        .push_maybe(threshold)
        .spacing(10),
    )
}

fn heirs<'a>(e: &TaliaEditor<'a>) -> Option<Element<'a, Message>> {
    let count = e.shape.heirs(e.paths);
    // A vault that can no longer change and has no heir: nothing to show.
    if e.locked && count == 0 {
        return None;
    }
    let months = e.shape.heir_months();
    let mut col = column![heading(
        t!("installer-talia-heirs"),
        t!("installer-talia-heirs-caption"),
    )]
    .spacing(10);

    for i in 0..count {
        let path_index = e.shape.heir_path(i);
        let path = &e.paths[path_index];
        let wait = months.get(i).copied();
        let picker: Element<'a, Message> = if e.locked {
            new::caption(wait.map(|m| Months(m).to_string()).unwrap_or_default()).into()
        } else {
            pick_list(
                HEIR_MONTHS.map(Months).collect::<Vec<_>>(),
                wait.map(Months),
                move |m: Months| talia(TaliaMessage::HeirMonths(i, m.0)),
            )
            .into()
        };
        let remove = (!e.locked).then(|| btn_remove(Some(talia(TaliaMessage::RemoveHeir(i)))));
        let head = row![
            p1_bold(t!("installer-talia-heir-number", number = i + 1)),
            Space::with_width(Length::Fill),
            new::caption(t!("installer-talia-heir-wait")).style(theme::text::secondary),
            picker,
        ]
        .push_maybe(remove)
        .spacing(10)
        .align_y(Alignment::Center);
        let warning = path
            .warning
            .map(|w| new::small_caption(w.message()).style(theme::text::error));
        col = col.push(
            card::simple(
                column![head]
                    .push_maybe(warning)
                    .push(key_slot(
                        path.keys.first().and_then(Option::as_ref),
                        HEIR,
                        t!("installer-talia-heir-key-number", number = i + 1),
                        path_index,
                        0,
                        e.use_taproot,
                    ))
                    .spacing(10),
            )
            .width(Length::Fill),
        );
    }

    if !e.locked && count < MAX_HEIRS {
        col = col.push(
            button::secondary(Some(icon::plus_icon()), t!("installer-talia-add-heir"))
                .on_press(talia(TaliaMessage::AddHeir)),
        );
    }
    Some(section(col))
}

fn rescue<'a>(e: &TaliaEditor<'a>) -> Element<'a, Message> {
    let template = e.shape.template;
    let heirs = e.shape.heirs(e.paths);
    let required =
        template != TaliaTemplate::SingleWithRescue && e.shape.rescue_required(heirs);

    // Everything there is to say about the Rescue Key, behind the info dot.
    let mut help = Vec::new();
    if required {
        help.push(format!("{}.", t!("installer-talia-rescue-required")));
        help.push(match template {
            TaliaTemplate::TwoOfTwo => t!("installer-talia-rescue-required-2-2"),
            _ => t!("installer-talia-rescue-required-2-3"),
        });
    }
    help.push(match template {
        TaliaTemplate::SingleWithRescue => t!("installer-talia-rescue-single"),
        _ => t!("installer-talia-rescue-caption"),
    });
    help.push(t!("installer-talia-rescue-opens", months = RESCUE_MONTHS));
    let mut col = column![heading(t!("installer-talia-rescue-key"), help.join(" "))].spacing(10);

    if template != TaliaTemplate::SingleWithRescue {
        if required {
            // Nothing to answer: the info dot says why.
        } else if !e.locked {
            col = col.push(
                row![
                    labelled_radio(
                        t!("installer-talia-rescue-yes"),
                        e.shape.rescue_answer == Some(true),
                        talia(TaliaMessage::Rescue(true)),
                    ),
                    labelled_radio(
                        t!("installer-talia-rescue-no"),
                        e.shape.rescue_answer == Some(false),
                        talia(TaliaMessage::Rescue(false)),
                    ),
                ]
                .spacing(HSpacing::XL),
            );
        }
    }

    if let Some(i) = e.shape.rescue_path_index(e.paths) {
        let path = &e.paths[i];
        col = col.push(our_slot(
            path.keys.first().and_then(Option::as_ref),
            t!("installer-talia-rescue-key"),
        ));
        if let Some(w) = path.warning {
            col = col.push(new::small_caption(w.message()).style(theme::text::error));
        }
    }
    section(col)
}

fn our_keys<'a>(e: &TaliaEditor<'a>) -> Element<'a, Message> {
    let mut col = column![heading(
        t!("installer-talia-keys-title"),
        t!("installer-talia-keys-caption"),
    )]
    .spacing(10);

    let get = |enabled: bool| {
        button::primary(None, t!("installer-talia-get-keys"))
            .on_press_maybe(enabled.then(|| talia(TaliaMessage::GetKeys)))
    };

    if !e.keys_available {
        col = col.push(
            new::caption(t!("installer-talia-keys-network-unsupported"))
                .style(theme::text::warning),
        );
        return section(col);
    }

    match e.flow {
        KeysFlow::Waiting { .. } => {
            col = col
                .push(new::caption(t!("installer-talia-keys-waiting")))
                .push(
                    row![
                        button::secondary(None, t!("installer-talia-keys-open-again"))
                            .on_press(talia(TaliaMessage::OpenKeysPage)),
                        button::tertiary(None, t!("btn-cancel"))
                            .on_press(talia(TaliaMessage::CancelKeys)),
                    ]
                    .spacing(HSpacing::M),
                );
        }
        KeysFlow::Done if e.locked => {
            col = col.push(
                row![
                    new::caption(t!("installer-talia-keys-done"))
                        .style(theme::text::success)
                        .width(Length::Fill),
                    button::tertiary(None, t!("installer-talia-keys-change-shape"))
                        .on_press(talia(TaliaMessage::ChangeShape)),
                ]
                .spacing(HSpacing::M)
                .align_y(Alignment::Center),
            );
        }
        flow => {
            if let KeysFlow::Failed(error) = flow {
                col = col.push(new::caption(error.clone()).style(theme::text::error));
            }
            let complete = e.shape.complete(e.shape.heirs(e.paths));
            if !complete {
                col = col.push(
                    new::caption(t!("installer-talia-keys-answer-rescue-first"))
                        .style(theme::text::secondary),
                );
            }
            col = col.push(get(complete));
        }
    }
    section(col)
}

pub fn talia_template<'a>(
    progress: (usize, usize),
    network: Network,
    e: TaliaEditor<'a>,
) -> Element<'a, Message> {
    let ours = e.shape.uses_21c(e.paths).then(|| our_keys(&e));
    let footer = super::template_footer(e.valid, e.processing, false);

    let content = column![
        descriptor_type(e.use_taproot, e.editing_descriptor_type),
        primary_path(&e)
    ]
        .push_maybe(heirs(&e))
        .push(rescue(&e))
        .push_maybe(ours)
        .push(Space::with_height(FOOTER_SPACING))
        .push(footer)
        .push(Space::with_height(BOTTOM_PADDING))
        .align_x(Alignment::Start)
        .spacing(VSpacing::L);

    layout(
        progress,
        network,
        None,
        t!("btn-set-keys"),
        content,
        Some(Message::Previous),
    )
}

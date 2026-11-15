use iced::{
    widget::{column, row, Space},
    Alignment,
};
use liana::miniscript::bitcoin::Network;

use liana_ui::{
    color,
    component::{
        button::{self, btn_add_recovery_option, btn_add_safety_net},
        text::{new, p1_bold},
        timeline::{spending_timeline, Template},
        tooltip::tooltip,
    },
    spacing::{HSpacing, VSpacing},
    theme,
    widget::*,
};

use crate::installer::{
    descriptor::Path,
    message::{self, Message, TaliaMessage},
    step::descriptor::editor::talia::KeysFlow,
    view::{
        editor::{
            defined_key, descriptor_type, our_key, path,
            template::{
                caption_block, row_next, BOTTOM_PADDING, DESCRIPTION_BOTTOM_PADDING, FOOTER_SPACING,
            },
            undefined_key,
        },
        layout,
    },
};
use crate::t;

pub fn custom_template_description(
    progress: (usize, usize),
    network: Network,
) -> Element<'static, Message> {
    let title = new::b1_bold(t!("installer-build-your-own"));

    let intro = caption_block(t!("installer-custom-template-description-1"));

    let explanation = caption_block(t!("installer-custom-template-description-2"));

    let diagram = spending_timeline(Template::Custom);

    let content = column![
        title,
        intro,
        explanation,
        diagram,
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

#[allow(clippy::too_many_arguments)]
pub fn custom_template<'a>(
    progress: (usize, usize),
    network: Network,
    use_taproot: bool,
    editing_descriptor_type: bool,
    primary_path: &'a Path,
    recovery_paths: &mut dyn Iterator<Item = (usize, &'a Path)>,
    safety_net_path: Option<(usize, &'a Path)>,
    num_non_primary_paths: usize,
    valid: bool,
    processing: bool,
    ours: Ours<'a>,
) -> Element<'a, Message> {
    let prim_keys_fixed = primary_path.keys.len() < 2; // can only delete a primary key if there are 2 or more

    let descriptor_type = descriptor_type(use_taproot, editing_descriptor_type);

    let primary = path(
        color::GREEN,
        Some(t!("installer-primary-spending-option")),
        primary_path.sequence,
        primary_path.warning,
        primary_path.threshold,
        primary_path
            .keys
            .iter()
            .enumerate()
            .map(|(i, primary_key)| {
                if ours.cosigner_slot(0) == Some(i) {
                    our_key(
                        primary_key.as_ref().map(|key| key.name.as_str()),
                        t!("installer-talia-cosigner"),
                        !prim_keys_fixed,
                    )
                } else if let Some(key) = primary_key {
                    defined_key(
                        &key.name,
                        color::GREEN,
                        t!("installer-primary-key"),
                        use_taproot.then(|| key.source.taproot_warning()).flatten(),
                        use_taproot.then(|| key.source.taproot_note()).flatten(),
                        prim_keys_fixed,
                    )
                } else {
                    undefined_key(
                        color::GREEN,
                        t!("installer-primary-key"),
                        !primary_path.keys[0..i].iter().any(|k| k.is_none()),
                        prim_keys_fixed,
                    )
                }
                .map(move |msg| message::DefinePath::Key(i, msg))
            })
            .collect(),
        false,
    )
    .map(|msg| Message::DefineDescriptor(message::DefineDescriptor::Path(0, msg)));

    let recovery_paths = recovery_paths.into_iter().enumerate().fold(
        column![].spacing(VSpacing::L),
        |col, (i, (p_idx, p))| {
            col.push(
                path(
                    color::ORANGE,
                    Some(t!("installer-recovery-option", number = i + 1)),
                    p.sequence,
                    p.warning,
                    p.threshold,
                    p.keys
                        .iter()
                        .enumerate()
                        .map(|(j, recovery_key)| {
                            // We cannot delete a key if doing so would remove all recovery paths,
                            // i.e. if there is only 1 recovery path and it contains only 1 key,
                            // and there is no safety net path.
                            let fixed = num_non_primary_paths < 2 && p.keys.len() < 2;
                            if ours.cosigner_slot(p_idx + 1) == Some(j) {
                                our_key(
                                    recovery_key.as_ref().map(|key| key.name.as_str()),
                                    t!("installer-talia-cosigner"),
                                    !fixed,
                                )
                            } else if let Some(key) = recovery_key {
                                defined_key(
                                    &key.name,
                                    color::ORANGE,
                                    t!("installer-recovery-key"),
                                    use_taproot.then(|| key.source.taproot_warning()).flatten(),
                                    use_taproot.then(|| key.source.taproot_note()).flatten(),
                                    fixed,
                                )
                            } else {
                                undefined_key(
                                    color::ORANGE,
                                    t!("installer-recovery-key"),
                                    !p.keys[0..j].iter().any(|k| k.is_none()),
                                    fixed,
                                )
                            }
                            .map(move |msg| message::DefinePath::Key(j, msg))
                        })
                        .collect(),
                    false,
                )
                .map(move |msg| {
                    Message::DefineDescriptor(message::DefineDescriptor::Path(
                        p_idx + 1, // add one to index to account for primary path.
                        msg,
                    ))
                }),
            )
        },
    );

    let add_recov_option = Some(Message::DefineDescriptor(
        message::DefineDescriptor::AddRecoveryPath,
    ));

    let safety_net =
        safety_net_path
            .is_none()
            .then_some(btn_add_safety_net(Some(Message::DefineDescriptor(
                message::DefineDescriptor::AddSafetyNetPath,
            ))));

    let btn_row = row![btn_add_recovery_option(add_recov_option), safety_net].spacing(HSpacing::M);

    let safety_net = safety_net_path.map(|(sn_index, sn_path)| {
        path(
            color::WHITE,
            Some(t!("installer-safety-net")),
            sn_path.sequence,
            sn_path.warning,
            sn_path.threshold,
            sn_path
                .keys
                .iter()
                .enumerate()
                .map(|(i, sn_key)| {
                    // Cannot delete safety net key if doing so would remove the safety net path
                    // and there are no other recovery paths.
                    let fixed = num_non_primary_paths < 2 && sn_path.keys.len() < 2;
                    if ours.rescue {
                        our_key(
                            sn_key.as_ref().map(|key| key.name.as_str()),
                            t!("installer-talia-rescue-key"),
                            !fixed,
                        )
                    } else if let Some(key) = sn_key {
                        defined_key(
                            &key.name,
                            color::WHITE,
                            t!("installer-safety-net-key"),
                            use_taproot.then(|| key.source.taproot_warning()).flatten(),
                            use_taproot.then(|| key.source.taproot_note()).flatten(),
                            fixed,
                        )
                    } else {
                        undefined_key(
                            color::WHITE,
                            t!("installer-safety-net-key"),
                            !sn_path.keys[0..i].iter().any(|k| k.is_none()),
                            fixed,
                        )
                    }
                    .map(move |msg| message::DefinePath::Key(i, msg))
                })
                .collect(),
            // The Rescue Key is one key: nothing to add next to it.
            true,
        )
        .map(move |msg| {
            // Add 1 to index to account for primary path.
            Message::DefineDescriptor(message::DefineDescriptor::Path(sn_index + 1, msg))
        })
    });

    let last_btn_row = super::template_footer(valid, processing, false);

    let content = column![
        descriptor_type,
        primary,
        recovery_paths,
        btn_row,
        safety_net,
        our_keys(&ours),
        Space::with_height(FOOTER_SPACING),
        last_btn_row,
        Space::with_height(BOTTOM_PADDING),
    ]
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

/// Build your own: the slots reserved for 21st Capital's keys and where
/// "Get my 21st Capital keys" stands.
pub struct Ours<'a> {
    /// Each (path, slot) reserved for our co-signer, one key for all of them.
    pub cosigner: Vec<(usize, usize)>,
    /// The Rescue Key path's slot is reserved for ours.
    pub rescue: bool,
    /// Every reserved slot holds its key.
    pub filled: bool,
    pub flow: &'a KeysFlow,
    /// 21st Capital issues keys on the wallet's network.
    pub keys_available: bool,
}

impl Ours<'_> {
    /// Our co-signer's slot on the path at `index`, when reserved.
    fn cosigner_slot(&self, index: usize) -> Option<usize> {
        self.cosigner
            .iter()
            .find(|(path, _)| *path == index)
            .map(|(_, slot)| *slot)
    }
}

/// "21st Capital keys": shown once a slot is reserved for one of ours, with
/// the one button that fetches them all.
fn our_keys<'a>(ours: &Ours<'a>) -> Option<Element<'a, Message>> {
    if ours.cosigner.is_empty() && !ours.rescue {
        return None;
    }
    let talia = |msg| Message::DefineDescriptor(message::DefineDescriptor::Talia(msg));

    let mut col = column![row![
        p1_bold(t!("installer-talia-keys-title")),
        tooltip(t!("installer-talia-keys-caption"))
    ]
    .spacing(6)
    .align_y(Alignment::Center)]
    .spacing(10);

    if !ours.keys_available {
        col = col.push(
            new::caption(t!("installer-talia-keys-network-unsupported"))
                .style(theme::text::warning),
        );
    } else {
        match ours.flow {
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
            KeysFlow::Done if ours.filled => {
                col = col.push(
                    new::caption(t!("installer-talia-keys-done")).style(theme::text::success),
                );
            }
            flow => {
                if let KeysFlow::Failed(error) = flow {
                    col = col.push(new::caption(error.clone()).style(theme::text::error));
                }
                col = col.push(
                    button::primary(None, t!("installer-talia-get-keys"))
                        .on_press(talia(TaliaMessage::GetKeys)),
                );
            }
        }
    }
    Some(
        Container::new(col)
            .padding(10)
            .width(iced::Length::Fill)
            .style(theme::card::border)
            .into(),
    )
}

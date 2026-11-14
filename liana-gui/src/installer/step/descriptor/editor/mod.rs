pub mod key;
pub mod talia;
pub mod template;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use iced::{Subscription, Task};
use liana::miniscript::bitcoin::bip32::ChildNumber;
use liana::{
    descriptors::{LianaDescriptor, PathInfo},
    miniscript::{
        bitcoin::{bip32::Fingerprint, Network},
        descriptor::DescriptorPublicKey,
    },
};

use liana_ui::{
    component::{form, spinner},
    widget::{modal::Modal, text_input, Element},
};

use crate::installer::KeySourceKind;
use crate::{
    app::settings::KeySetting,
    hw::HardwareWallets,
    installer::{
        context::{CompileInputs, DescriptorTemplate},
        descriptor::{Key, Path, PathKind, PathSequence, PathWarning},
        message::{self, Message},
        step::{Context, Step},
        view,
    },
    signer::Signer,
    t,
};
use liana_connect::keys::api::KeyKind;

use key::{new_multixkey_from_xpub, EditKeyAlias, PathData, SelectKeySource, ALIAS_INPUT_ID};
use talia::{KeysFlow, TaliaShape, TaliaTemplate};

use crate::installer::{
    message::TaliaMessage,
    talia_keys::{self, listener::BrowserPages, proof::default_pins, Endpoints, FetchedKeys},
};

pub trait DescriptorEditModal {
    fn processing(&self) -> bool {
        false
    }
    fn update(&mut self, _hws: &mut HardwareWallets, _message: Message) -> Task<Message> {
        Task::none()
    }
    fn view<'a>(&'a self, _hws: &'a HardwareWallets) -> Element<'a, Message>;
    fn subscription(&self, _hws: &HardwareWallets) -> Subscription<Message> {
        Subscription::none()
    }
}

pub struct DefineDescriptor {
    network: Network,
    use_taproot: bool,
    editing_descriptor_type: bool,

    modal: Option<Box<dyn DescriptorEditModal>>,
    signer: Arc<Mutex<Signer>>,

    keys: HashMap<Fingerprint, Key>,
    paths: Vec<Path>,
    accounts: HashMap<Fingerprint, ChildNumber>,
    descriptor_template: DescriptorTemplate,

    error: Option<String>,
    processing: bool,
    compiled: Option<LianaDescriptor>,

    /// The client's choices when the template is one of the iPhone app's.
    talia: Option<TaliaShape>,
    keys_flow: KeysFlow,
    keys_attempt: u64,
    /// The 21st Capital keys placed in their slots.
    our_keys: Vec<Fingerprint>,
}

impl DefineDescriptor {
    pub fn new(network: Network, signer: Arc<Mutex<Signer>>) -> Self {
        Self {
            network,
            use_taproot: true,
            editing_descriptor_type: false,
            modal: None,

            signer,
            error: None,
            keys: HashMap::new(),
            descriptor_template: DescriptorTemplate::default(),
            paths: Vec::new(),
            accounts: Default::default(),
            processing: false,
            compiled: None,
            talia: None,
            keys_flow: KeysFlow::Idle,
            keys_attempt: 0,
            our_keys: Vec::new(),
        }
    }

    fn check_for_warning(&mut self) {
        let mut all_sequence = HashSet::new();
        let mut duplicate_sequences = HashSet::new();
        for path in &mut self.paths {
            if all_sequence.contains(&path.sequence.as_u16()) {
                duplicate_sequences.insert(path.sequence.as_u16());
            } else {
                all_sequence.insert(path.sequence.as_u16());
            }
        }
        for path in &mut self.paths {
            if duplicate_sequences.contains(&path.sequence.as_u16()) {
                path.warning = Some(PathWarning::DuplicateSequence);
            } else if path.keys.iter().all(|key| {
                // All keys must be Some for warning to apply.
                key.as_ref()
                    .is_some_and(|k| k.source.provider_key_kind() == Some(KeyKind::Cosigner))
            }) {
                path.warning = Some(PathWarning::OnlyCosignerKeys);
            } else if path
                .keys
                .iter()
                .flatten() // can ignore None
                .any(|key| !path.kind().can_choose_key_source_kind(&key.source.kind()))
            {
                path.warning = Some(PathWarning::KeySourceKindDisallowed);
            } else {
                path.warning = None;
            }
        }
    }

    fn valid(&self) -> bool {
        if let Some(shape) = &self.talia {
            if !shape.complete(shape.heirs(&self.paths)) {
                return false;
            }
        }
        !self.paths.iter().any(|path| {
            !path.valid()
                || (self.use_taproot
                    && path
                        .keys
                        .iter()
                        .any(|k| !k.as_ref().is_some_and(|k| k.source.is_compatible_taproot())))
        }) && self.paths.len() >= 2
    }

    fn check_setup(&mut self) {
        self.check_for_warning();
        self.compiled = None;
    }

    fn load_template(&mut self, template: DescriptorTemplate) {
        if self.descriptor_template != template || self.paths.is_empty() {
            self.keys_flow = KeysFlow::Idle;
            // Our keys belong to the shape they were fetched for.
            for fingerprint in self.our_keys.drain(..) {
                self.keys.remove(&fingerprint);
            }
            match TaliaTemplate::from_template(template) {
                Some(talia) => {
                    let shape = TaliaShape::new(talia);
                    self.paths = shape.initial_paths();
                    self.talia = Some(shape);
                    // Every vault holding a 21st Capital key or a timelock is taproot.
                    self.use_taproot = true;
                    self.editing_descriptor_type = false;
                }
                None => {
                    self.talia = None;
                    self.paths = vec![Path::new_primary_path(), Path::new_recovery_path()];
                }
            }
            self.check_for_warning();
        }
        self.descriptor_template = template;
    }

    /// Whether the shape may still change: not once the website was asked
    /// for keys of this shape, nor while it is being asked.
    fn talia_locked(&self) -> bool {
        self.keys_flow.is_waiting() || self.talia_keys_placed()
    }

    /// Whether a 21st Capital key sits in one of its slots.
    fn talia_keys_placed(&self) -> bool {
        let Some(shape) = &self.talia else {
            return false;
        };
        let cosigner = shape
            .cosigner_slot(&self.paths)
            .is_some_and(|j| self.paths[0].keys[j].is_some());
        let rescue = shape
            .rescue_path_index(&self.paths)
            .is_some_and(|i| self.paths[i].keys[0].is_some());
        cosigner || rescue
    }

    /// Takes the 21st Capital keys out of their slots.
    fn remove_talia_keys(&mut self, hws: &mut HardwareWallets) {
        let Some(shape) = &self.talia else {
            return;
        };
        let mut slots = Vec::new();
        if let Some(j) = shape.cosigner_slot(&self.paths) {
            slots.push((0, j));
        }
        if let Some(i) = shape.rescue_path_index(&self.paths) {
            slots.push((i, 0));
        }
        for (i, j) in slots {
            if let Some(key) = self.paths[i].keys[j].take() {
                self.keys.remove(&key.fingerprint);
                hws.aliases.remove(&key.fingerprint);
            }
        }
        self.our_keys.clear();
        self.keys_flow = KeysFlow::Idle;
        self.check_setup();
    }

    /// Puts the fetched keys in their slots, our keys last on every path.
    fn place_talia_keys(
        &mut self,
        hws: &mut HardwareWallets,
        fetched: FetchedKeys,
    ) -> Result<(), String> {
        let Some(shape) = &self.talia else {
            return Err(t!("installer-talia-keys-missing"));
        };
        let mut placements = Vec::new();
        match (shape.cosigner_slot(&self.paths), fetched.cosigner) {
            (Some(j), Some(key)) => placements.push(((0, j), key)),
            (Some(_), None) => return Err(t!("installer-talia-keys-missing")),
            _ => {}
        }
        match (shape.rescue_path_index(&self.paths), fetched.rescue) {
            (Some(i), Some(key)) => placements.push(((i, 0), key)),
            (Some(_), None) => return Err(t!("installer-talia-keys-missing")),
            _ => {}
        }
        // One of ours must never be one of the client's keys too.
        let in_vault = |fingerprint: &Fingerprint| {
            self.paths
                .iter()
                .flat_map(|path| path.keys.iter().flatten())
                .any(|key| key.fingerprint == *fingerprint)
        };
        if placements.iter().any(|(_, key)| in_vault(&key.fingerprint)) {
            return Err(t!("installer-talia-keys-already-used"));
        }
        for ((i, j), key) in placements {
            self.keys.insert(key.fingerprint, key.clone());
            hws.aliases.insert(key.fingerprint, key.name.clone());
            self.our_keys.push(key.fingerprint);
            self.paths[i].keys[j] = Some(key);
        }
        self.check_setup();
        Ok(())
    }

    fn update_talia(&mut self, hws: &mut HardwareWallets, message: TaliaMessage) -> Task<Message> {
        let locked = self.talia_locked();
        let Some(shape) = self.talia.as_mut() else {
            return Task::none();
        };
        match message {
            TaliaMessage::CosignerOurs(ours) if !locked && shape.cosigner_21c != ours => {
                shape.cosigner_21c = ours;
                // The slot changes hands: whatever key it held goes.
                if let Some(slot) = self.paths[0].keys.last_mut() {
                    *slot = None;
                }
            }
            TaliaMessage::Rescue(include) if !locked => {
                shape.set_rescue(&mut self.paths, include);
            }
            TaliaMessage::AddHeir if !locked => shape.add_heir(&mut self.paths),
            TaliaMessage::RemoveHeir(i) if !locked => shape.remove_heir(&mut self.paths, i),
            TaliaMessage::HeirMonths(i, months) if !locked => {
                shape.set_heir_months(&mut self.paths, i, months);
            }
            TaliaMessage::GetKeys
                if !locked
                    && shape.complete(shape.heirs(&self.paths))
                    && shape.uses_21c(&self.paths) =>
            {
                return self.get_talia_keys()
            }
            TaliaMessage::OpenKeysPage => {
                if let KeysFlow::Waiting { url, .. } = &self.keys_flow {
                    return Task::done(Message::OpenUrl(url.clone()));
                }
            }
            TaliaMessage::CancelKeys => {
                if self.keys_flow.is_waiting() {
                    self.keys_flow = KeysFlow::Idle;
                }
            }
            TaliaMessage::KeysFetched(attempt, res) => {
                let current = matches!(
                    &self.keys_flow,
                    KeysFlow::Waiting { attempt: a, .. } if *a == attempt
                );
                if current {
                    self.keys_flow = match res {
                        Ok(fetched) => match self.place_talia_keys(hws, *fetched) {
                            Ok(()) => KeysFlow::Done,
                            Err(e) => KeysFlow::Failed(e),
                        },
                        Err(e) => KeysFlow::Failed(e.message()),
                    };
                }
            }
            TaliaMessage::ChangeShape => self.remove_talia_keys(hws),
            _ => {}
        }
        self.check_for_warning();
        Task::none()
    }

    /// Opens the website and waits for the keys to come back.
    fn get_talia_keys(&mut self) -> Task<Message> {
        let Some(shape) = &self.talia else {
            return Task::none();
        };
        let vault_shape = shape.vault_shape(&self.paths);
        let pages = BrowserPages {
            title: t!("installer-talia-browser-title"),
            done: t!("installer-talia-browser-done"),
            wrong_request: t!("installer-talia-browser-wrong-request"),
        };
        let started = Endpoints::from_env().and_then(|endpoints| {
            talia_keys::start(self.network, vault_shape, pages, endpoints, default_pins())
        });
        match started {
            Err(e) => {
                self.keys_flow = KeysFlow::Failed(e.message());
                Task::none()
            }
            Ok(started) => {
                self.keys_attempt += 1;
                let attempt = self.keys_attempt;
                let (wait, handle) = Task::perform(started.keys, move |res| {
                    Message::DefineDescriptor(message::DefineDescriptor::Talia(
                        TaliaMessage::KeysFetched(attempt, res.map(Box::new)),
                    ))
                })
                .abortable();
                self.keys_flow = KeysFlow::Waiting {
                    attempt,
                    url: started.url.clone(),
                    _abort: handle.abort_on_drop(),
                };
                Task::batch([Task::done(Message::OpenUrl(started.url)), wait])
            }
        }
    }

    fn keys(&self) -> HashMap<Fingerprint, (Vec<(usize, usize)>, Key)> {
        let mut keys: HashMap<Fingerprint, (Vec<(usize, usize)>, Key)> = self
            .keys
            .clone()
            .into_iter()
            .map(|(fg, key)| (fg, (vec![], key)))
            .collect();
        for (i, path) in self.paths.iter().enumerate() {
            for (j, key) in path.keys.iter().enumerate() {
                if let Some(k) = key {
                    let fg = k.fingerprint;
                    if !keys.contains_key(&fg) {
                        tracing::error!("DefineDesctriptor::keys() key {} missing", fg);
                    } else {
                        let entry = keys.get_mut(&fg).expect("checked");
                        entry.0.push((i, j));
                    }
                }
            }
        }
        keys
    }

    fn path_keys(&self, paths: Vec<usize>) -> Vec<Fingerprint> {
        let mut out = vec![];
        for path in paths {
            let mut fgs: Vec<_> = self.paths[path]
                .keys
                .iter()
                .filter_map(|k| k.as_ref().map(|k| k.fingerprint))
                .collect();
            out.append(&mut fgs);
        }
        out
    }

    fn edit_key_modal(
        &self,
        path_kind: PathKind,
        coordinates: Vec<(usize, usize)>,
    ) -> SelectKeySource {
        let mut token_kind = vec![];
        if let PathKind::SafetyNet = path_kind {
            token_kind.push(KeyKind::SafetyNet);
        }
        if path_kind.can_choose_key_source_kind(&KeySourceKind::Token(KeyKind::Cosigner))
            && path_kind != PathKind::SafetyNet
        {
            token_kind.push(KeyKind::Cosigner);
        }
        // Our templates take 21st Capital's keys, never another provider's.
        if self.talia.is_some() {
            token_kind.clear();
        }
        let mut keys = self.path_keys(coordinates.iter().map(|c| c.0).collect());
        // 21st Capital's keys are never offered for the client's own slots.
        keys.extend(self.our_keys.iter().copied());
        let actual_path = PathData {
            coordinates,
            keys,
            token_kind,
        };
        let keys = self.keys();
        SelectKeySource::new(
            self.network,
            actual_path,
            keys,
            self.accounts.clone(),
            self.signer.clone(),
            self.use_taproot,
        )
    }
}

impl Step for DefineDescriptor {
    fn load_context(&mut self, ctx: &Context) {
        self.load_template(ctx.descriptor_template);
        // Drop any transient state from a previous visit (failed compile,
        // in-flight task we navigated away from, stale mailbox).
        self.error = None;
        self.processing = false;
        self.compiled = None;
    }
    // form value is set as valid each time it is edited.
    // Verification of the values is happening when the user click on Next button.
    fn update(&mut self, hws: &mut HardwareWallets, message: Message) -> Task<Message> {
        self.error = None;
        match message {
            Message::Close => {
                self.modal = None;
            }
            Message::CreateTaprootDescriptor(use_taproot) => {
                self.use_taproot = use_taproot;
                self.check_setup();
            }
            Message::ShowDescriptorTypeOptions(show) => {
                self.editing_descriptor_type = show;
            }
            Message::DefineDescriptor(message::DefineDescriptor::ChangeTemplate(template)) => {
                self.descriptor_template = template;
                if TaliaTemplate::from_template(template).is_none() {
                    // Customized: the paths stay, as Liana's own editor has them.
                    self.talia = None;
                    self.keys_flow = KeysFlow::Idle;
                }
            }
            Message::DefineDescriptor(message::DefineDescriptor::AddRecoveryPath) => {
                self.paths.push(Path::new_recovery_path());
            }
            Message::DefineDescriptor(message::DefineDescriptor::AddSafetyNetPath) => {
                if !self.paths.iter().any(|p| p.kind() == PathKind::SafetyNet) {
                    self.paths.push(Path::new_safety_net_path());
                }
            }
            Message::DefineDescriptor(message::DefineDescriptor::KeysEdited(coordinates, key)) => {
                match key {
                    key::SelectedKey::Existing(fingerprint) => {
                        if let Some(existing_key) = self.keys.get(&fingerprint) {
                            if !self.keys.contains_key(&fingerprint) {
                                tracing::error!("Key {fingerprint} does not exists");
                                return Task::none();
                            }
                            for coordinate in coordinates {
                                if !self.paths[coordinate.0]
                                    .keys
                                    .contains(&Some(existing_key.clone()))
                                {
                                    self.paths[coordinate.0].keys[coordinate.1] =
                                        Some(existing_key.clone());
                                } else {
                                    tracing::error!(
                                        "Key {fingerprint} already in path {}",
                                        coordinate.0
                                    );
                                }
                            }
                            self.check_setup();
                        } else {
                            tracing::error!("Key with fingerprint {fingerprint} does not exists");
                        }
                        self.modal = None;
                    }
                    key::SelectedKey::New(key) => {
                        if let Some(acc) = key.account {
                            self.accounts.insert(key.fingerprint, acc);
                        }
                        if self.keys.contains_key(&key.fingerprint) {
                            tracing::error!("Key {} already exists", key.fingerprint);
                        }
                        self.keys.insert(key.fingerprint, *key.clone());
                        hws.aliases.insert(key.fingerprint, key.name.clone());
                        for coordinate in coordinates {
                            if !self.paths[coordinate.0].keys.contains(&Some(*key.clone())) {
                                self.paths[coordinate.0].keys[coordinate.1] = Some(*key.clone());
                            } else {
                                tracing::error!(
                                    "Key {} already in path {}",
                                    key.fingerprint,
                                    coordinate.0
                                );
                            }
                        }
                        self.check_setup();
                        self.modal = None;
                    }
                    key::SelectedKey::None => {
                        self.modal = None;
                    }
                }
            }
            Message::DefineDescriptor(message::DefineDescriptor::KeysEdit(
                path_kind,
                coordinates,
            )) => {
                let modal = self.edit_key_modal(path_kind, coordinates);
                self.modal = Some(Box::new(modal));
            }
            Message::DefineDescriptor(message::DefineDescriptor::AliasEdited(fg, alias)) => {
                if let Some(key) = self.keys.get_mut(&fg) {
                    key.name = alias.clone();
                }
                hws.aliases.insert(fg, alias.clone());
                // FIXME: this is a bad hack, we should not store duplicates of keys in paths,
                // source of truth should be self.keys
                for p in &mut self.paths {
                    #[allow(clippy::manual_flatten)]
                    for k in &mut p.keys {
                        if let Some(key) = k {
                            if key.fingerprint == fg {
                                key.name = alias.clone();
                            }
                        }
                    }
                }
                self.modal = None;
            }
            Message::DefineDescriptor(message::DefineDescriptor::Reset) => {
                hws.aliases.clear();
                self.keys.clear();
                self.paths.clear();
                self.load_template(self.descriptor_template);
                self.modal = None;
            }
            Message::DefineDescriptor(message::DefineDescriptor::Talia(msg)) => {
                return self.update_talia(hws, msg);
            }
            Message::DefineDescriptor(message::DefineDescriptor::Path(i, msg)) => {
                match msg {
                    message::DefinePath::SequenceEdited(seq) => {
                        self.modal = None;
                        if let Some(Path {
                            sequence: PathSequence::Recovery(s),
                            ..
                        }) = self.paths.get_mut(i)
                        {
                            *s = seq;
                        }
                        self.check_for_warning();
                    }
                    message::DefinePath::ThresholdEdited(t) => {
                        self.modal = None;
                        if let Some(path) = self.paths.get_mut(i) {
                            path.threshold = t;
                        }
                    }
                    message::DefinePath::EditSequence => {
                        if let Some(path) = self.paths.get(i) {
                            self.modal = Some(Box::new(EditSequenceModal::new(i, path.sequence)));
                        }
                    }
                    message::DefinePath::EditThreshold => {
                        if let Some(path) = self.paths.get(i) {
                            self.modal = Some(Box::new(EditThresholdModal::new(
                                i,
                                (path.threshold, path.keys.len()),
                            )));
                        }
                    }

                    message::DefinePath::AddKey => {
                        if let Some(path) = self.paths.get_mut(i) {
                            path.keys.push(None);
                            path.threshold += 1;
                            self.check_for_warning();
                        }
                    }
                    message::DefinePath::Key(j, msg) => {
                        match msg {
                            message::DefineKey::Clipboard(key) => {
                                return Task::perform(async move { key }, Message::Clipboard);
                            }
                            message::DefineKey::OpenUrl(url) => {
                                return Task::done(Message::OpenUrl(url));
                            }

                            message::DefineKey::EditAlias => {
                                let coordinates = vec![(i, j)];
                                let path_kind = self.paths[i].sequence.into();
                                if let Some(path) = self.paths.get(i) {
                                    if let Some(Some(key)) = path.keys.get(j) {
                                        let fg = key.fingerprint;
                                        let alias = key.name.clone();
                                        let modal = EditKeyAlias::new(
                                            self.keys(),
                                            fg,
                                            alias,
                                            path_kind,
                                            coordinates,
                                        );
                                        self.modal = Some(Box::new(modal));
                                        return text_input::focus(ALIAS_INPUT_ID);
                                    }
                                }
                                log::error!("DefineDescriptor.update(): DefineKey::EditAlias => key is None.");
                                return Task::none();
                            }
                            message::DefineKey::Edit => {
                                let coordinates = vec![(i, j)];
                                let path_kind = self.paths[i].sequence.into();
                                let modal = self.edit_key_modal(path_kind, coordinates);
                                self.modal = Some(Box::new(modal));
                            }
                            message::DefineKey::Delete => {
                                if let Some(path) = self.paths.get_mut(i) {
                                    path.keys.remove(j);
                                    if path.threshold > path.keys.len() {
                                        path.threshold -= 1;
                                    }
                                }
                                // Only delete non-primary paths.
                                if i > 0 // we could alternatively check `path_kind != PathKind::Primary`
                                && self
                                    .paths
                                    .get(i)
                                    .map(|path| path.keys.is_empty())
                                    .unwrap_or(false)
                                {
                                    self.paths.remove(i);
                                }
                                self.check_setup();
                            }
                        }
                    }
                }
            }
            Message::DefineDescriptor(message::DefineDescriptor::Compiled(res)) => {
                self.processing = false;
                match res {
                    Ok(desc) => {
                        self.compiled = Some(*desc);
                        return Task::done(Message::Next);
                    }
                    Err(e) => {
                        self.error = Some(e);
                    }
                }
            }
            Message::EditKeyAlias(msg) => match msg.clone() {
                key::EditKeyAliasMessage::Alias(_)
                | key::EditKeyAliasMessage::Save
                | key::EditKeyAliasMessage::Replace => {
                    if let Some(modal) = &mut self.modal {
                        return modal.update(hws, Message::EditKeyAlias(msg));
                    }
                }
                key::EditKeyAliasMessage::DoReplace {
                    path_kind,
                    coordinates,
                } => {
                    let modal = self.edit_key_modal(path_kind, coordinates);
                    self.modal = Some(Box::new(modal));
                }
                key::EditKeyAliasMessage::Close => self.modal = None,
            },
            _ => {
                if let Some(modal) = &mut self.modal {
                    return modal.update(hws, message);
                }
            }
        };
        Task::none()
    }

    fn subscription(&self, hws: &HardwareWallets) -> Subscription<Message> {
        if let Some(modal) = &self.modal {
            modal.subscription(hws)
        } else {
            Subscription::none()
        }
    }

    fn apply(&mut self, ctx: &mut Context) -> bool {
        if self.paths.len() < 2 {
            return false;
        }

        ctx.bitcoin_config.network = self.network;
        ctx.keys = HashMap::new();
        let mut hw_is_used = false;
        let mut spending_keys: Vec<DescriptorPublicKey> = Vec::new();
        let mut key_derivation_index = HashMap::<Fingerprint, usize>::new();
        for spending_key in self.paths[0].keys.iter().clone() {
            let fingerprint = spending_key
                .as_ref()
                .expect("Must be present at this step")
                .fingerprint;
            let key = self
                .keys
                .get(&fingerprint)
                .expect("Must be present at this step");
            if let DescriptorPublicKey::XPub(xpub) = &key.key {
                if let Some((master_fingerprint, _)) = xpub.origin {
                    ctx.keys.insert(
                        master_fingerprint,
                        KeySetting {
                            master_fingerprint,
                            name: key.name.clone(),
                            provider_key: key.source.provider_key(),
                        },
                    );
                    if key.source.device_kind().is_some() {
                        hw_is_used = true;
                    }
                }
                let derivation_index = key_derivation_index.get(&fingerprint).unwrap_or(&0);
                spending_keys.push(DescriptorPublicKey::MultiXPub(new_multixkey_from_xpub(
                    xpub.clone(),
                    *derivation_index,
                )));
                key_derivation_index.insert(fingerprint, derivation_index + 1);
            }
        }

        let mut recovery_paths = BTreeMap::new();

        for path in &self.paths[1..] {
            let mut recovery_keys: Vec<DescriptorPublicKey> = Vec::new();
            for recovery_key in path.keys.iter().clone() {
                let fingerprint = recovery_key
                    .as_ref()
                    .expect("Must be present at this step")
                    .fingerprint;
                let key = self
                    .keys
                    .get(&fingerprint)
                    .expect("Must be present at this step");
                if let DescriptorPublicKey::XPub(xpub) = &key.key {
                    if let Some((master_fingerprint, _)) = xpub.origin {
                        ctx.keys.insert(
                            master_fingerprint,
                            KeySetting {
                                master_fingerprint,
                                name: key.name.clone(),
                                provider_key: key.source.provider_key(),
                            },
                        );
                        if key.source.device_kind().is_some() {
                            hw_is_used = true;
                        }
                    }

                    let derivation_index = key_derivation_index.get(&fingerprint).unwrap_or(&0);
                    recovery_keys.push(DescriptorPublicKey::MultiXPub(new_multixkey_from_xpub(
                        xpub.clone(),
                        *derivation_index,
                    )));
                    key_derivation_index.insert(fingerprint, derivation_index + 1);
                }
            }

            let recovery_keys = if recovery_keys.len() == 1 {
                PathInfo::Single(recovery_keys[0].clone())
            } else {
                PathInfo::Multi(path.threshold, recovery_keys)
            };

            recovery_paths.insert(path.sequence.as_u16(), recovery_keys);
        }

        if spending_keys.is_empty() {
            return false;
        }

        let primary = if spending_keys.len() == 1 {
            PathInfo::Single(spending_keys[0].clone())
        } else {
            PathInfo::Multi(self.paths[0].threshold, spending_keys)
        };

        if let Some(desc) = self.compiled.take() {
            ctx.descriptor = Some(desc);
            ctx.hw_is_used = hw_is_used;
            return true;
        }

        ctx.pending_compile = Some(CompileInputs {
            primary,
            recovery: recovery_paths,
            use_taproot: self.use_taproot,
        });
        self.processing = true;
        false
    }

    fn view<'a>(
        &'a self,
        hws: &'a HardwareWallets,
        progress: (usize, usize),
        network: Network,
        _email: Option<&'a str>,
    ) -> Element<'a, Message> {
        let content = match (&self.talia, self.descriptor_template) {
            (Some(shape), _) => view::editor::template::talia::talia_template(
                progress,
                network,
                view::editor::template::talia::TaliaEditor {
                    shape,
                    paths: &self.paths,
                    flow: &self.keys_flow,
                    locked: self.talia_locked(),
                    keys_available: talia_keys::contract_network(self.network).is_some(),
                    valid: self.valid(),
                    processing: self.processing,
                },
            ),
            (None, _) => view::editor::template::custom::custom_template(
                progress,
                network,
                self.use_taproot,
                self.editing_descriptor_type,
                &self.paths[0],
                &mut self.paths[1..]
                    .iter()
                    .enumerate()
                    .filter(|(_, p)| p.kind() == PathKind::Recovery),
                self.paths[1..]
                    .iter()
                    .enumerate()
                    .find(|(_, p)| p.kind() == PathKind::SafetyNet),
                self.paths[1..].len(),
                self.valid(),
                self.processing,
            ),
        };
        let modal: Option<Element<'a, Message>> = if self.processing {
            Some(spinner::spinner_modal(
                t!("common-loading"),
                t!("installer-descriptor-generating"),
            ))
        } else {
            self.modal.as_ref().map(|modal| modal.view(hws))
        };

        let on_blur = (!self.processing)
            .then_some(self.modal.as_ref())
            .flatten()
            .and_then(|modal| (!modal.processing()).then_some(Message::Close));

        let backdrop = if self.processing {
            liana_ui::color::BLACK_60
        } else {
            liana_ui::color::BLACK_80
        };

        Modal::with_optional(content, modal)
            .on_blur(on_blur)
            .backdrop(backdrop)
            .into()
    }
}

impl From<DefineDescriptor> for Box<dyn Step> {
    fn from(s: DefineDescriptor) -> Box<dyn Step> {
        Box::new(s)
    }
}

pub struct EditSequenceModal {
    path_index: usize,
    sequence: form::Value<String>,
}

impl EditSequenceModal {
    pub fn new(path_index: usize, path_sequence: PathSequence) -> Self {
        Self {
            path_index,
            sequence: form::Value {
                value: path_sequence.as_u16().to_string(),
                warning: None,
                valid: true,
            },
        }
    }
}

impl DescriptorEditModal for EditSequenceModal {
    fn processing(&self) -> bool {
        false
    }

    fn update(&mut self, _hws: &mut HardwareWallets, message: Message) -> Task<Message> {
        if let Message::DefineDescriptor(message::DefineDescriptor::ThresholdSequenceModal(msg)) =
            message
        {
            match msg {
                message::ThresholdSequenceModal::SequenceEdited(seq) => {
                    if let Ok(s) = u16::from_str(&seq) {
                        self.sequence.valid = s != 0
                    } else {
                        self.sequence.valid = false;
                    }
                    self.sequence.value = seq;
                }
                message::ThresholdSequenceModal::Confirm => {
                    if self.sequence.valid {
                        if let Ok(sequence) = u16::from_str(&self.sequence.value) {
                            let path_index = self.path_index;
                            return Task::perform(
                                async move { (path_index, sequence) },
                                |(path_index, sequence)| {
                                    message::DefineDescriptor::Path(
                                        path_index,
                                        message::DefinePath::SequenceEdited(sequence),
                                    )
                                },
                            )
                            .map(Message::DefineDescriptor);
                        }
                    }
                }
                _ => {}
            }
        }
        Task::none()
    }

    fn view(&self, _hws: &HardwareWallets) -> Element<'_, Message> {
        view::editor::edit_sequence_modal(&self.sequence)
    }
}

pub struct EditThresholdModal {
    threshold: (usize, usize),
    path_index: usize,
}

impl EditThresholdModal {
    pub fn new(path_index: usize, threshold: (usize, usize)) -> Self {
        Self {
            threshold,
            path_index,
        }
    }
}

impl DescriptorEditModal for EditThresholdModal {
    fn processing(&self) -> bool {
        false
    }

    fn update(&mut self, _hws: &mut HardwareWallets, message: Message) -> Task<Message> {
        if let Message::DefineDescriptor(message::DefineDescriptor::ThresholdSequenceModal(msg)) =
            message
        {
            match msg {
                message::ThresholdSequenceModal::ThresholdEdited(threshold) => {
                    if threshold <= self.threshold.1 {
                        self.threshold.0 = threshold;
                    }
                }
                message::ThresholdSequenceModal::Confirm => {
                    let path_index = self.path_index;
                    let threshold = self.threshold.0;
                    return Task::perform(
                        async move { (path_index, threshold) },
                        |(path_index, threshold)| {
                            message::DefineDescriptor::Path(
                                path_index,
                                message::DefinePath::ThresholdEdited(threshold),
                            )
                        },
                    )
                    .map(Message::DefineDescriptor);
                }
                _ => {}
            }
        }
        Task::none()
    }

    fn view(&self, _hws: &HardwareWallets) -> Element<'_, Message> {
        view::editor::edit_threshold_modal(self.threshold)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::futures::StreamExt;
    use iced_runtime::{task::into_stream, Action};
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    use crate::installer::step::descriptor::editor::key::{SelectKeySource, SelectedKey};
    use crate::{dir::LianaDirectory, installer::descriptor::KeySource};
    use liana::descriptors::LianaPolicy;

    pub struct Sandbox<S: Step> {
        step: Arc<Mutex<S>>,
    }

    impl<S: Step + 'static> Sandbox<S> {
        pub fn new(step: S) -> Self {
            Self {
                step: Arc::new(Mutex::new(step)),
            }
        }

        pub fn check<F: FnOnce(&mut S)>(&self, check: F) {
            let mut step = self.step.lock().unwrap();
            check(&mut step)
        }

        pub fn check_apply(&self, ctx: &mut Context) -> bool {
            self.step.lock().unwrap().apply(ctx)
        }

        pub async fn update(&self, message: Message) {
            let mut hws = HardwareWallets::new(
                LianaDirectory::new(PathBuf::from_str("/").unwrap()),
                Network::Bitcoin,
            );
            let cmd = self.step.lock().unwrap().update(&mut hws, message);
            if let Some(mut stream) = into_stream(cmd) {
                while let Some(action) = stream.next().await {
                    if let Action::Output(msg) = action {
                        let _cmd = self.step.lock().unwrap().update(&mut hws, msg);
                    }
                }
            }
        }
        pub async fn load(&self, ctx: &Context) {
            self.step.lock().unwrap().load_context(ctx);
        }
    }

    // Drive `apply()` end-to-end like the real installer does on
    // `Message::Next`: try the direct path, and if compilation was deferred to
    // a `CompileInputs` task, run it synchronously and feed `Compiled(...)`
    // back into the step so the second `apply()` returns `true`.
    async fn run_apply(sandbox: &Sandbox<DefineDescriptor>, ctx: &mut Context) -> bool {
        if sandbox.check_apply(ctx) {
            return true;
        }
        let Some(inputs) = ctx.pending_compile.take() else {
            return false;
        };
        let CompileInputs {
            primary,
            recovery,
            use_taproot,
        } = inputs;
        let policy = if use_taproot {
            LianaPolicy::new(primary, recovery)
        } else {
            LianaPolicy::new_legacy(primary, recovery)
        }
        .expect("test inputs should compile");
        let desc = LianaDescriptor::new(policy);
        sandbox
            .update(Message::DefineDescriptor(
                message::DefineDescriptor::Compiled(Ok(Box::new(desc))),
            ))
            .await;
        sandbox.check_apply(ctx)
    }

    #[tokio::test]
    async fn test_define_descriptor_use_hotkey() {
        let mut ctx = Context::new(
            Network::Signet,
            LianaDirectory::new(PathBuf::from_str("/").unwrap()),
            crate::installer::context::RemoteBackend::None,
        );
        // One primary key and one recovery key, as "Build your own" starts.
        ctx.descriptor_template = DescriptorTemplate::Custom;
        let sandbox: Sandbox<DefineDescriptor> = Sandbox::new(DefineDescriptor::new(
            Network::Signet,
            Arc::new(Mutex::new(Signer::generate(Network::Bitcoin).unwrap())),
        ));
        sandbox.load(&ctx).await;

        // Edit primary key
        sandbox
            .update(Message::DefineDescriptor(message::DefineDescriptor::Path(
                0,
                message::DefinePath::Key(0, message::DefineKey::Edit),
            )))
            .await;
        sandbox.check(|step| assert!(step.modal.is_some()));
        sandbox
            .update(SelectKeySource::route(
                key::SelectKeySourceMessage::SelectGenerateHotKey,
            ))
            .await;
        sandbox
            .update(SelectKeySource::route(key::SelectKeySourceMessage::Alias(
                "hot_signer_key".to_string(),
            )))
            .await;
        sandbox
            .update(SelectKeySource::route(key::SelectKeySourceMessage::Next))
            .await;
        sandbox.check(|step| assert!(step.modal.is_none()));

        // Edit sequence
        sandbox
            .update(Message::DefineDescriptor(message::DefineDescriptor::Path(
                1,
                message::DefinePath::SequenceEdited(1000),
            )))
            .await;

        // Edit recovery key
        sandbox
            .update(Message::DefineDescriptor(message::DefineDescriptor::Path(
                1,
                message::DefinePath::Key(0, message::DefineKey::Edit),
            )))
            .await;
        sandbox.check(|step| assert!(step.modal.is_some()));
        sandbox
            .update(SelectKeySource::route(
                key::SelectKeySourceMessage::SelectEnterXpub,
            ))
            .await;
        sandbox
            .update(SelectKeySource::route(
                key::SelectKeySourceMessage::Xpub("[f5acc2fd/48'/1'/0'/2']tpubDFAqEGNyad35aBCKUAXbQGDjdVhNueno5ZZVEn3sQbW5ci457gLR7HyTmHBg93oourBssgUxuWz1jX5uhc1qaqFo9VsybY1J5FuedLfm4dK".to_string())
            ))
            .await;
        sandbox
            .update(SelectKeySource::route(key::SelectKeySourceMessage::Alias(
                "External recovery key".to_string(),
            )))
            .await;
        sandbox
            .update(SelectKeySource::route(key::SelectKeySourceMessage::Next))
            .await;
        sandbox.check(|step| assert!(step.modal.is_none()));
        assert!(run_apply(&sandbox, &mut ctx).await);
        sandbox.check(|step| {
            assert!(ctx
                .descriptor
                .as_ref()
                .unwrap()
                .to_string()
                .contains(&step.signer.lock().unwrap().fingerprint().to_string()));
        });
    }

    #[tokio::test]
    async fn test_define_descriptor_stores_if_hw_is_used() {
        let mut ctx = Context::new(
            Network::Testnet,
            LianaDirectory::new(PathBuf::from_str("/").unwrap()),
            crate::installer::context::RemoteBackend::None,
        );
        // One primary key and one recovery key, as "Build your own" starts.
        ctx.descriptor_template = DescriptorTemplate::Custom;
        let sandbox: Sandbox<DefineDescriptor> = Sandbox::new(DefineDescriptor::new(
            Network::Testnet,
            Arc::new(Mutex::new(Signer::generate(Network::Testnet).unwrap())),
        ));
        sandbox.load(&ctx).await;

        let key = DescriptorPublicKey::from_str("[4df3f0e3/84'/0'/0']tpubDDRs9DnRUiJc4hq92PSJKhfzQBgHJUrDo7T2i48smsDfLsQcm3Vh7JhuGqJv8zozVkNFin8YPgpmn2NWNmpRaE3GW2pSxbmAzYf2juy7LeW").unwrap();
        let specter_key = Key {
            name: "My Specter key".to_string(),
            fingerprint: key.master_fingerprint(),
            key,
            source: KeySource::Device(async_hwi::DeviceKind::Specter, None),
            account: None,
        };

        // Use Specter device for primary key
        sandbox
            .update(Message::DefineDescriptor(
                message::DefineDescriptor::KeysEdited(
                    vec![(0, 0)],
                    SelectedKey::New(Box::new(specter_key.clone())),
                ),
            ))
            .await;

        // Edit recovery key
        sandbox
            .update(Message::DefineDescriptor(message::DefineDescriptor::Path(
                1,
                message::DefinePath::Key(0, message::DefineKey::Edit),
            )))
            .await;
        sandbox.check(|step| assert!(step.modal.is_some()));
        sandbox
            .update(SelectKeySource::route(
                key::SelectKeySourceMessage::SelectEnterXpub,
            ))
            .await;
        sandbox
            .update(SelectKeySource::route(
                key::SelectKeySourceMessage::Xpub("[f5acc2fd/48'/1'/0'/2']tpubDFAqEGNyad35aBCKUAXbQGDjdVhNueno5ZZVEn3sQbW5ci457gLR7HyTmHBg93oourBssgUxuWz1jX5uhc1qaqFo9VsybY1J5FuedLfm4dK".to_string())
            ))
            .await;
        sandbox
            .update(SelectKeySource::route(key::SelectKeySourceMessage::Alias(
                "External recovery key".to_string(),
            )))
            .await;
        sandbox
            .update(SelectKeySource::route(key::SelectKeySourceMessage::Next))
            .await;
        sandbox.check(|step| assert!(step.modal.is_none()));
        assert!(run_apply(&sandbox, &mut ctx).await);
        assert!(ctx.hw_is_used);

        // Now edit primary key to use hot signer instead of Specter device
        sandbox
            .update(Message::DefineDescriptor(message::DefineDescriptor::Path(
                0,
                message::DefinePath::Key(0, message::DefineKey::Edit),
            )))
            .await;
        sandbox.check(|step| assert!(step.modal.is_some()));
        sandbox
            .update(SelectKeySource::route(
                key::SelectKeySourceMessage::SelectGenerateHotKey,
            ))
            .await;
        sandbox
            .update(SelectKeySource::route(key::SelectKeySourceMessage::Alias(
                "hot signer key".to_string(),
            )))
            .await;
        sandbox
            .update(SelectKeySource::route(key::SelectKeySourceMessage::Next))
            .await;
        sandbox.check(|step| assert!(step.modal.is_none()));
        assert!(run_apply(&sandbox, &mut ctx).await);
        assert!(!ctx.hw_is_used);

        // Now edit the recovery key to use Specter device
        sandbox
            .update(Message::DefineDescriptor(
                message::DefineDescriptor::KeysEdited(
                    vec![(1, 0)],
                    SelectedKey::New(Box::new(specter_key.clone())),
                ),
            ))
            .await;
        assert!(run_apply(&sandbox, &mut ctx).await);
        assert!(ctx.hw_is_used);
    }

    /// A key of a throwaway test seed at 48'/1'/0'/2', entered as an xpub.
    fn test_key(seed: u8, name: &str) -> Key {
        use liana::miniscript::bitcoin::{
            bip32::{DerivationPath, Xpriv, Xpub},
            secp256k1::Secp256k1,
            NetworkKind,
        };
        let secp = Secp256k1::new();
        let master = Xpriv::new_master(NetworkKind::Test, &[seed; 32]).unwrap();
        let path = DerivationPath::from_str("m/48'/1'/0'/2'").unwrap();
        let xpub = Xpub::from_priv(&secp, &master.derive_priv(&secp, &path).unwrap());
        let fingerprint = master.fingerprint(&secp);
        let key =
            DescriptorPublicKey::from_str(&format!("[{fingerprint}/48'/1'/0'/2']{xpub}")).unwrap();
        Key {
            source: KeySource::Manual,
            name: name.to_string(),
            fingerprint,
            key,
            account: None,
        }
    }

    fn talia_message(msg: TaliaMessage) -> Message {
        Message::DefineDescriptor(message::DefineDescriptor::Talia(msg))
    }

    fn set_key(path: usize, index: usize, key: &Key) -> Message {
        Message::DefineDescriptor(message::DefineDescriptor::KeysEdited(
            vec![(path, index)],
            SelectedKey::New(Box::new(key.clone())),
        ))
    }

    /// Puts the step in the state "Get my 21st Capital keys" leaves it in
    /// while it waits, for attempt `attempt`.
    fn waiting(step: &mut DefineDescriptor, attempt: u64) {
        let (_, handle) = Task::<Message>::none().abortable();
        step.keys_attempt = attempt;
        step.keys_flow = KeysFlow::Waiting {
            attempt,
            url: "https://www.21stcapital.com/desktop/keys".to_string(),
            _abort: handle,
        };
    }

    fn fetched(cosigner: Option<&Key>, rescue: Option<&Key>) -> Message {
        talia_message(TaliaMessage::KeysFetched(
            1,
            Ok(Box::new(FetchedKeys {
                network: talia_keys::proof::ProofNet::Testnet4,
                vault_number: Some(1),
                account: Some(1000),
                cosigner: cosigner.cloned(),
                rescue: rescue.cloned(),
            })),
        ))
    }

    fn talia_sandbox(template: DescriptorTemplate) -> (Context, Sandbox<DefineDescriptor>) {
        let mut ctx = Context::new(
            Network::Testnet4,
            LianaDirectory::new(PathBuf::from_str("/").unwrap()),
            crate::installer::context::RemoteBackend::None,
        );
        ctx.descriptor_template = template;
        let sandbox = Sandbox::new(DefineDescriptor::new(
            Network::Testnet4,
            Arc::new(Mutex::new(Signer::generate(Network::Testnet4).unwrap())),
        ));
        (ctx, sandbox)
    }

    #[tokio::test]
    async fn a_two_of_three_with_an_heir_and_our_keys() {
        let (mut ctx, sandbox) = talia_sandbox(DescriptorTemplate::TwoOfThree);
        sandbox.load(&ctx).await;
        let (mine1, mine2, heir) = (
            test_key(1, "Mine 1"),
            test_key(2, "Mine 2"),
            test_key(3, "Heir"),
        );
        let (cosigner, rescue) = (
            test_key(4, "21st Capital co-signer"),
            test_key(5, "21st Capital Rescue Key"),
        );

        sandbox.update(talia_message(TaliaMessage::AddHeir)).await;
        sandbox.check(|step| {
            // The Rescue Key is a question now that an heir is the way back.
            assert!(!step.valid());
            assert_eq!(step.paths.len(), 2);
        });
        sandbox
            .update(talia_message(TaliaMessage::Rescue(true)))
            .await;
        sandbox
            .update(talia_message(TaliaMessage::HeirMonths(0, 9)))
            .await;
        sandbox.update(set_key(0, 0, &mine1)).await;
        sandbox.update(set_key(0, 1, &mine2)).await;
        sandbox.update(set_key(1, 0, &heir)).await;
        sandbox.check(|step| {
            assert_eq!(step.paths.len(), 3);
            // Our two slots are still empty.
            assert!(!step.valid());
            assert_eq!(
                step.talia.as_ref().unwrap().vault_shape(&step.paths),
                talia_keys::VaultShape {
                    config: talia_keys::VaultConfig::Multi2of3,
                    cosigner: true,
                    rescue: true,
                    heirs: vec![9],
                }
            );
            waiting(step, 1);
        });

        // A stale answer changes nothing; the current one fills our slots.
        sandbox
            .update(talia_message(TaliaMessage::KeysFetched(
                0,
                Err(talia_keys::HandoffError::TimedOut),
            )))
            .await;
        sandbox.check(|step| assert!(step.keys_flow.is_waiting()));
        sandbox
            .update(fetched(Some(&cosigner), Some(&rescue)))
            .await;
        sandbox.check(|step| {
            assert!(matches!(step.keys_flow, KeysFlow::Done));
            assert!(step.talia_locked());
            assert!(step.valid());
        });

        // The shape no longer changes under our keys.
        sandbox.update(talia_message(TaliaMessage::AddHeir)).await;
        sandbox
            .update(talia_message(TaliaMessage::Rescue(false)))
            .await;
        sandbox.check(|step| assert_eq!(step.paths.len(), 3));

        assert!(run_apply(&sandbox, &mut ctx).await);
        let desc = ctx.descriptor.as_ref().unwrap();
        assert!(desc.is_taproot());
        let text = desc.to_string();
        let at = |key: &Key| text.find(&key.fingerprint.to_string()).unwrap();
        // The client's keys first, ours last.
        assert!(at(&mine1) < at(&mine2) && at(&mine2) < at(&cosigner));
        let policy = desc.policy();
        assert!(matches!(policy.primary_path(), PathInfo::Multi(2, keys) if keys.len() == 3));
        let waits: Vec<u16> = policy.recovery_paths().keys().copied().collect();
        assert_eq!(waits, vec![39_447, 65_535]);
        let (_, heir_origins) = policy.recovery_paths()[&39_447].thresh_origins();
        assert!(heir_origins.contains_key(&heir.fingerprint));
        let (_, rescue_origins) = policy.recovery_paths()[&65_535].thresh_origins();
        assert!(rescue_origins.contains_key(&rescue.fingerprint));
        assert_eq!(
            ctx.keys[&cosigner.fingerprint].name,
            "21st Capital co-signer"
        );
        assert_eq!(
            ctx.keys[&rescue.fingerprint].name,
            "21st Capital Rescue Key"
        );
    }

    #[tokio::test]
    async fn changing_the_shape_takes_our_keys_out() {
        let (ctx, sandbox) = talia_sandbox(DescriptorTemplate::TwoOfTwo);
        sandbox.load(&ctx).await;
        let (cosigner, rescue) = (test_key(4, "co"), test_key(5, "rescue"));
        sandbox.check(|step| waiting(step, 1));
        sandbox
            .update(fetched(Some(&cosigner), Some(&rescue)))
            .await;
        sandbox.check(|step| {
            assert!(step.talia_keys_placed());
            assert!(step.keys.contains_key(&rescue.fingerprint));
        });
        sandbox
            .update(talia_message(TaliaMessage::ChangeShape))
            .await;
        sandbox.check(|step| {
            assert!(!step.talia_keys_placed());
            assert!(!step.talia_locked());
            assert!(!step.keys.contains_key(&cosigner.fingerprint));
            assert!(step.paths.iter().flat_map(|p| &p.keys).all(Option::is_none));
        });
        // Unlocked: an heir can be added again.
        sandbox.update(talia_message(TaliaMessage::AddHeir)).await;
        sandbox.check(|step| assert_eq!(step.talia.as_ref().unwrap().heirs(&step.paths), 1));
    }

    #[tokio::test]
    async fn our_keys_never_take_the_place_of_the_clients() {
        let (ctx, sandbox) = talia_sandbox(DescriptorTemplate::SingleWithRescue);
        sandbox.load(&ctx).await;
        let mine = test_key(1, "Mine");
        sandbox.update(set_key(0, 0, &mine)).await;
        sandbox.check(|step| waiting(step, 1));
        // The backend hands back the client's own key as the Rescue Key.
        sandbox.update(fetched(None, Some(&mine))).await;
        sandbox.check(|step| {
            assert!(matches!(step.keys_flow, KeysFlow::Failed(_)));
            assert!(!step.talia_keys_placed());
        });
        // A Rescue Key that did not come back is a failure too.
        sandbox.check(|step| waiting(step, 1));
        sandbox
            .update(fetched(Some(&test_key(9, "co")), None))
            .await;
        sandbox.check(|step| assert!(matches!(step.keys_flow, KeysFlow::Failed(_))));
    }

    #[tokio::test]
    async fn a_self_held_co_signer_needs_no_21c_key() {
        let (mut ctx, sandbox) = talia_sandbox(DescriptorTemplate::TwoOfTwo);
        sandbox.load(&ctx).await;
        sandbox
            .update(talia_message(TaliaMessage::CosignerOurs(false)))
            .await;
        sandbox.update(talia_message(TaliaMessage::AddHeir)).await;
        sandbox
            .update(talia_message(TaliaMessage::Rescue(false)))
            .await;
        for (slot, seed) in [((0, 0), 1u8), ((0, 1), 2), ((1, 0), 3)] {
            sandbox
                .update(set_key(slot.0, slot.1, &test_key(seed, "k")))
                .await;
        }
        sandbox.check(|step| {
            assert!(!step.talia.as_ref().unwrap().uses_21c(&step.paths));
            assert!(step.valid());
        });
        assert!(run_apply(&sandbox, &mut ctx).await);
        let policy = ctx.descriptor.as_ref().unwrap().policy();
        assert_eq!(
            policy.recovery_paths().keys().copied().collect::<Vec<_>>(),
            vec![52_596]
        );
    }

    #[tokio::test]
    async fn heirs_on_the_same_wait_are_refused_by_liana() {
        let (ctx, sandbox) = talia_sandbox(DescriptorTemplate::TwoOfThree);
        sandbox.load(&ctx).await;
        sandbox.update(talia_message(TaliaMessage::AddHeir)).await;
        sandbox.update(talia_message(TaliaMessage::AddHeir)).await;
        sandbox
            .update(talia_message(TaliaMessage::HeirMonths(1, 12)))
            .await;
        sandbox.check(|step| {
            assert_eq!(step.paths[1].warning, Some(PathWarning::DuplicateSequence));
            assert_eq!(step.paths[2].warning, Some(PathWarning::DuplicateSequence));
        });
        sandbox
            .update(talia_message(TaliaMessage::HeirMonths(1, 15)))
            .await;
        sandbox.check(|step| assert!(step.paths.iter().all(|p| p.warning.is_none())));
    }
}

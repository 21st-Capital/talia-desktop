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

/// Whether two asks are for the same vault. The layout only records what the
/// layers looked like when the keys were asked for: editing a layer later
/// (a wait, a key of the client's) never takes our keys out, since each new
/// fetch takes another of the client's vault slots.
fn same_vault(a: Option<&talia_keys::VaultShape>, b: Option<&talia_keys::VaultShape>) -> bool {
    let strip = |shape: &talia_keys::VaultShape| talia_keys::VaultShape {
        layout: None,
        ..shape.clone()
    };
    a.map(strip) == b.map(strip)
}

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
    /// Build your own: the layers whose last slot is reserved for our
    /// co-signer, any layer but the Rescue Key's (decided 2026-11-15).
    custom_cosigner: Vec<usize>,
    /// Build your own: the co-signer key fetched, the same on every layer.
    custom_cosigner_key: Option<Fingerprint>,
    /// Build your own: the Rescue Key path's slot is reserved for ours.
    custom_rescue: bool,
    /// Build your own: what the website was asked for, while it is open.
    custom_asked: Option<talia_keys::VaultShape>,
    /// Build your own: what the keys in the reserved slots were fetched for.
    custom_fetched: Option<talia_keys::VaultShape>,
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
            custom_cosigner: Vec::new(),
            custom_cosigner_key: None,
            custom_rescue: false,
            custom_asked: None,
            custom_fetched: None,
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
        // Build your own: 21st Capital's co-signer never signs alone, so a
        // layer holding it needs at least 2 signatures (decided 2026-11-15).
        if self.talia.is_none() {
            for &i in &self.custom_cosigner {
                if let Some(path) = self.paths.get_mut(i) {
                    if path.warning.is_none() && path.threshold < 2 {
                        path.warning = Some(PathWarning::OurCosignerAlone);
                    }
                }
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
            // Build your own: new paths, nothing reserved for ours yet.
            self.custom_cosigner.clear();
            self.custom_cosigner_key = None;
            self.custom_rescue = false;
            self.custom_asked = None;
            self.custom_fetched = None;
            match TaliaTemplate::from_template(template) {
                Some(talia) => {
                    let shape = TaliaShape::new(talia);
                    self.paths = shape.initial_paths();
                    self.talia = Some(shape);
                    // Taproot unless the client picks SegWit (a Jade cannot
                    // sign a taproot vault), as in Liana's own templates.
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
        let backup = shape
            .backup_21c_slot()
            .is_some_and(|(i, j)| self.paths[i].keys[j].is_some());
        cosigner || rescue || backup
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
        slots.extend(shape.backup_21c_slot());
        for (i, j) in slots {
            if let Some(key) = self.paths[i].keys[j].take() {
                self.keys.remove(&key.fingerprint);
                hws.aliases.remove(&key.fingerprint);
            }
        }
        // The backup way in's copy of the primary path's keys follows.
        shape.sync_backup_keys(&mut self.paths);
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
        // Our co-signer on the primary path and as the added key of the
        // backup way in: one key for both.
        let mut cosigner_slots: Vec<(usize, usize)> = shape
            .cosigner_slot(&self.paths)
            .map(|j| (0, j))
            .into_iter()
            .collect();
        cosigner_slots.extend(shape.backup_21c_slot());
        if !cosigner_slots.is_empty() {
            let Some(key) = fetched.cosigner else {
                return Err(t!("installer-talia-keys-missing"));
            };
            for slot in cosigner_slots {
                placements.push((slot, key.clone()));
            }
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
            if !self.our_keys.contains(&key.fingerprint) {
                self.our_keys.push(key.fingerprint);
            }
            self.paths[i].keys[j] = Some(key);
        }
        if let Some(shape) = &self.talia {
            shape.sync_backup_keys(&mut self.paths);
        }
        self.check_setup();
        Ok(())
    }

    fn update_talia(&mut self, hws: &mut HardwareWallets, message: TaliaMessage) -> Task<Message> {
        if self.talia.is_none() {
            return self.update_custom_ours(hws, message);
        }
        let locked = self.talia_locked();
        let Some(shape) = self.talia.as_mut() else {
            return Task::none();
        };
        match message {
            TaliaMessage::CosignerOurs(ours) if !locked && shape.cosigner_21c != ours => {
                let backup_holder = shape.backup_holder_21c();
                shape.cosigner_21c = ours;
                // The slot changes hands: whatever key it held goes.
                if let Some(slot) = self.paths[0].keys.last_mut() {
                    *slot = None;
                }
                // So does the backup way in's added key when its holder changes
                // with it (our co-signer in the vault makes the third key the
                // client's).
                if shape.backup_holder_21c() != backup_holder {
                    if let Some((i, j)) = shape.backup_added_slot() {
                        self.paths[i].keys[j] = None;
                    }
                }
            }
            TaliaMessage::Backup(include) if !locked => {
                shape.set_backup(&mut self.paths, include);
            }
            TaliaMessage::BackupKind(kind) if !locked => {
                shape.set_backup_kind(&mut self.paths, kind);
            }
            TaliaMessage::BackupHolderOurs(ours) if !locked => {
                shape.set_backup_holder(&mut self.paths, ours);
            }
            TaliaMessage::BackupMonths(months) if !locked => {
                shape.set_backup_months(&mut self.paths, months);
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

    /// Build your own: our co-signer and Rescue Key. Picking one only
    /// reserves its slot; "Get my 21st Capital keys" then fetches every
    /// reserved one from our website at once, as the templates do.
    fn update_custom_ours(
        &mut self,
        hws: &mut HardwareWallets,
        message: TaliaMessage,
    ) -> Task<Message> {
        match message {
            TaliaMessage::OurKey(i, j) if !self.keys_flow.is_waiting() => {
                self.modal = None;
                self.reserve_custom_slot(i, j);
            }
            TaliaMessage::GetKeys if !self.keys_flow.is_waiting() => {
                if let Some(shape) = self.custom_shape() {
                    let task = self.start_keys_fetch(shape.clone());
                    if self.keys_flow.is_waiting() {
                        self.custom_asked = Some(shape);
                    }
                    return task;
                }
            }
            TaliaMessage::OpenKeysPage => {
                if let KeysFlow::Waiting { url, .. } = &self.keys_flow {
                    return Task::done(Message::OpenUrl(url.clone()));
                }
            }
            TaliaMessage::CancelKeys => {
                if self.keys_flow.is_waiting() {
                    self.keys_flow = KeysFlow::Idle;
                    self.custom_asked = None;
                }
            }
            TaliaMessage::KeysFetched(attempt, res) => {
                let current = matches!(
                    &self.keys_flow,
                    KeysFlow::Waiting { attempt: a, .. } if *a == attempt
                );
                if current {
                    let asked = self.custom_asked.take();
                    self.keys_flow = match res {
                        Ok(fetched) => match self.place_custom_keys(hws, asked, *fetched) {
                            Ok(()) => KeysFlow::Done,
                            Err(e) => KeysFlow::Failed(e),
                        },
                        Err(e) => KeysFlow::Failed(e.message()),
                    };
                }
            }
            _ => {}
        }
        Task::none()
    }

    /// The Rescue Key's path (Liana's Safety Net path), if the vault has one.
    fn rescue_path_index(&self) -> Option<usize> {
        self.paths
            .iter()
            .position(|path| path.kind() == PathKind::SafetyNet)
    }

    /// Build your own: the layers holding our co-signer, each in its last
    /// slot. One key for all of them.
    fn custom_cosigner_paths(&self) -> &[usize] {
        if self.talia.is_some() {
            return &[];
        }
        &self.custom_cosigner
    }

    /// Build your own: our co-signer's slot on layer `i`, when reserved.
    fn custom_cosigner_slot(&self, i: usize) -> Option<usize> {
        if !self.custom_cosigner_paths().contains(&i) {
            return None;
        }
        self.paths.get(i)?.keys.len().checked_sub(1)
    }

    /// Build your own: the Rescue Key path, when its slot is reserved for ours.
    fn custom_rescue_path(&self) -> Option<usize> {
        if self.talia.is_some() || !self.custom_rescue {
            return None;
        }
        self.rescue_path_index()
    }

    /// Build your own: whether every slot reserved for ours holds its key.
    fn custom_ours_filled(&self) -> bool {
        let cosigner = self.custom_cosigner_paths().iter().all(|&i| {
            self.paths
                .get(i)
                .and_then(|path| path.keys.last())
                .is_some_and(|key| key.is_some())
        });
        let rescue = self
            .custom_rescue_path()
            .is_none_or(|i| self.paths[i].keys.first().is_some_and(|key| key.is_some()));
        cosigner && rescue
    }

    /// Build your own: what to ask the website for, every key reserved. A
    /// vault that is one of the website's types (a primary path of 1 key, 2 of
    /// 2 or 2 of 3, and at most our Rescue Key besides) is asked for as that
    /// type; any other is "custom" with its layout, layer by layer (decided
    /// 2026-11-15).
    fn custom_shape(&self) -> Option<talia_keys::VaultShape> {
        let cosigner = !self.custom_cosigner_paths().is_empty();
        let rescue = self.custom_rescue_path().is_some();
        if !cosigner && !rescue {
            return None;
        }
        let primary = self.paths.first()?;
        let rescue_path = self.custom_rescue_path();
        let primary_only = (1..self.paths.len()).all(|i| Some(i) == rescue_path);
        let config = match (primary_only, primary.keys.len(), primary.threshold) {
            (true, 1, _) => talia_keys::VaultConfig::Single,
            (true, 2, 2) => talia_keys::VaultConfig::Multi2of2,
            (true, 3, 2) => talia_keys::VaultConfig::Multi2of3,
            _ => talia_keys::VaultConfig::Custom,
        };
        let layout = (config == talia_keys::VaultConfig::Custom).then(|| self.custom_layout());
        Some(talia_keys::VaultShape {
            config,
            cosigner,
            rescue,
            // The client's own recovery paths are not heirs to us.
            heirs: Vec::new(),
            backup: None,
            layout,
        })
    }

    /// Build your own: the vault layer by layer, in the order the layers
    /// open. The client's keys are numbered as they first appear, so a key
    /// on several layers keeps its number; a slot not filled yet counts as a
    /// key of its own. Never a key, fingerprint or name.
    fn custom_layout(&self) -> talia_keys::Layout {
        use talia_keys::{LayoutKey, LayoutLayer};
        let rescue_path = self.custom_rescue_path();
        let mut own: Vec<Option<Fingerprint>> = Vec::new();
        let mut layers: Vec<LayoutLayer> = self
            .paths
            .iter()
            .enumerate()
            .map(|(i, path)| {
                let cosigner_slot = self.custom_cosigner_slot(i);
                let keys = path
                    .keys
                    .iter()
                    .enumerate()
                    .map(|(j, key)| {
                        if Some(j) == cosigner_slot {
                            return LayoutKey::Cosigner;
                        }
                        if Some(i) == rescue_path && j == 0 {
                            return LayoutKey::Rescue;
                        }
                        let fingerprint = key.as_ref().map(|k| k.fingerprint);
                        let known =
                            fingerprint.and_then(|f| own.iter().position(|o| *o == Some(f)));
                        LayoutKey::Own(match known {
                            Some(n) => n + 1,
                            None => {
                                own.push(fingerprint);
                                own.len()
                            }
                        })
                    })
                    .collect();
                LayoutLayer {
                    after_blocks: path.sequence.as_u16(),
                    need: path.threshold,
                    keys,
                }
            })
            .collect();
        layers.sort_by_key(|layer| layer.after_blocks);
        talia_keys::Layout { layers }
    }

    /// Build your own: reserves the slot (i, j) for one of our keys.
    fn reserve_custom_slot(&mut self, i: usize, j: usize) {
        if Some(i) == self.rescue_path_index() {
            if !self.custom_rescue {
                if let Some(slot) = self.paths[i].keys.first_mut() {
                    *slot = None;
                }
                self.custom_rescue = true;
            }
        } else if let Some(path) = self.paths.get_mut(i) {
            let Some(last) = path.keys.len().checked_sub(1) else {
                return;
            };
            // Once per layer, next to at least one of the client's keys.
            if j > last || last == 0 || self.custom_cosigner.contains(&i) {
                return;
            }
            // Ours sits last: the client's keys come first.
            path.keys.swap(j, last);
            path.keys[last] = None;
            self.custom_cosigner.push(i);
            self.custom_cosigner.sort_unstable();
        }
        self.check_setup();
    }

    /// Build your own: keeps our keys true to what is reserved. A key fetched
    /// for another shape (another vault type, a co-signer or Rescue Key added
    /// or given back) leaves its slot, so one press of "Get my 21st Capital
    /// keys" fetches one vault's pair again. A layer given our co-signer after
    /// the keys came takes the same co-signer key (decided 2026-11-15).
    fn sync_custom_ours(&mut self, hws: &mut HardwareWallets) {
        if self.talia.is_some() {
            return;
        }
        let mut changed = false;
        if self.custom_rescue && self.rescue_path_index().is_none() {
            self.custom_rescue = false;
        }
        // Our co-signer only on layers with room for the client's keys too.
        let paths = &self.paths;
        self.custom_cosigner.retain(|&i| {
            paths
                .get(i)
                .is_some_and(|p| p.kind() != PathKind::SafetyNet && p.keys.len() >= 2)
        });
        // Our keys sit in the slots reserved for them and nowhere else.
        let rescue_path = self.custom_rescue_path();
        let (our_keys, cosigner_paths) = (&self.our_keys, &self.custom_cosigner);
        for (i, path) in self.paths.iter_mut().enumerate() {
            let cosigner_slot = cosigner_paths.contains(&i).then(|| path.keys.len() - 1);
            for (j, key) in path.keys.iter_mut().enumerate() {
                let ours = key
                    .as_ref()
                    .is_some_and(|k| our_keys.contains(&k.fingerprint));
                let reserved = Some(j) == cosigner_slot || (Some(i) == rescue_path && j == 0);
                if ours && !reserved {
                    *key = None;
                    changed = true;
                }
            }
        }
        if self.custom_fetched.is_some()
            && !same_vault(self.custom_fetched.as_ref(), self.custom_shape().as_ref())
        {
            self.remove_custom_keys(hws);
        }
        if let Some(key) = self
            .custom_cosigner_key
            .and_then(|fingerprint| self.keys.get(&fingerprint))
            .cloned()
        {
            for &i in &self.custom_cosigner {
                if let Some(slot) = self.paths[i].keys.last_mut() {
                    if slot.is_none() {
                        *slot = Some(key.clone());
                        changed = true;
                    }
                }
            }
        }
        if changed {
            self.check_setup();
        } else {
            // A threshold edit changes no key: the warnings still follow it.
            self.check_for_warning();
        }
    }

    /// Build your own: takes our keys out of their slots.
    fn remove_custom_keys(&mut self, hws: &mut HardwareWallets) {
        for fingerprint in self.our_keys.drain(..) {
            self.keys.remove(&fingerprint);
            hws.aliases.remove(&fingerprint);
            for path in &mut self.paths {
                for key in &mut path.keys {
                    if key.as_ref().is_some_and(|k| k.fingerprint == fingerprint) {
                        *key = None;
                    }
                }
            }
        }
        self.custom_fetched = None;
        self.custom_cosigner_key = None;
        if matches!(self.keys_flow, KeysFlow::Done) {
            self.keys_flow = KeysFlow::Idle;
        }
        self.check_setup();
    }

    /// Build your own: puts the fetched keys in the reserved slots, the one
    /// co-signer key on every layer reserved for it.
    fn place_custom_keys(
        &mut self,
        hws: &mut HardwareWallets,
        asked: Option<talia_keys::VaultShape>,
        fetched: FetchedKeys,
    ) -> Result<(), String> {
        let Some(asked) = asked else {
            return Err(t!("installer-talia-keys-missing"));
        };
        // The vault must still be the one the keys were asked for.
        if !same_vault(self.custom_shape().as_ref(), Some(&asked)) {
            return Err(t!("installer-talia-keys-vault-changed"));
        }
        let mut placements = Vec::new();
        let cosigner_slots: Vec<(usize, usize)> = self
            .custom_cosigner_paths()
            .iter()
            .filter_map(|&i| Some((i, self.custom_cosigner_slot(i)?)))
            .collect();
        let mut cosigner_key = None;
        if !cosigner_slots.is_empty() {
            let Some(key) = fetched.cosigner else {
                return Err(t!("installer-talia-keys-missing"));
            };
            cosigner_key = Some(key.fingerprint);
            for slot in cosigner_slots {
                placements.push((slot, key.clone()));
            }
        }
        if let Some(i) = self.custom_rescue_path() {
            let Some(key) = fetched.rescue else {
                return Err(t!("installer-talia-keys-missing"));
            };
            placements.push(((i, 0), key));
        }
        // One of ours must never be one of the client's keys too.
        let clients = |fingerprint: &Fingerprint| {
            !self.our_keys.contains(fingerprint)
                && self
                    .paths
                    .iter()
                    .flat_map(|path| path.keys.iter().flatten())
                    .any(|key| key.fingerprint == *fingerprint)
        };
        if placements.iter().any(|(_, key)| clients(&key.fingerprint)) {
            return Err(t!("installer-talia-keys-already-used"));
        }
        // The new pair replaces any earlier one of ours.
        self.remove_custom_keys(hws);
        for ((i, j), key) in placements {
            self.keys.insert(key.fingerprint, key.clone());
            hws.aliases.insert(key.fingerprint, key.name.clone());
            if !self.our_keys.contains(&key.fingerprint) {
                self.our_keys.push(key.fingerprint);
            }
            self.paths[i].keys[j] = Some(key);
        }
        self.custom_fetched = Some(asked);
        self.custom_cosigner_key = cosigner_key;
        self.check_setup();
        Ok(())
    }

    /// Opens the website and waits for the keys to come back.
    fn get_talia_keys(&mut self) -> Task<Message> {
        let Some(shape) = &self.talia else {
            return Task::none();
        };
        let vault_shape = shape.vault_shape();
        self.start_keys_fetch(vault_shape)
    }

    /// Opens the website asking for `vault_shape`'s keys and waits for them.
    fn start_keys_fetch(&mut self, vault_shape: talia_keys::VaultShape) -> Task<Message> {
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
        let actual_path = self.path_data(path_kind, coordinates);
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

    /// What the key window offers for the slots at `coordinates`.
    fn path_data(&self, path_kind: PathKind, coordinates: Vec<(usize, usize)>) -> PathData {
        // Talia Desktop takes 21st Capital's keys, never a key provider's
        // token: no "Enter a Cosigner token" / "Safety Net token" (decided
        // 2026-11-15). Build your own offers our co-signer on any layer but the
        // Rescue Key's, and our Rescue Key on its own path instead, fetched
        // from our website.
        let token_kind = vec![];
        let ours = if self.talia.is_some()
            || talia_keys::contract_network(self.network).is_none()
            || coordinates.len() != 1
        {
            None
        } else {
            let i = coordinates[0].0;
            match path_kind {
                PathKind::SafetyNet if !self.custom_rescue => Some(key::OurKeyOffer::Rescue),
                PathKind::SafetyNet => None,
                // Once per layer, next to at least one key of the client's.
                _ => (self.paths.get(i).is_some_and(|p| p.keys.len() >= 2)
                    && !self.custom_cosigner.contains(&i))
                .then_some(key::OurKeyOffer::Cosigner),
            }
        };
        let manual_only = path_kind == PathKind::SafetyNet;
        let mut keys = self.path_keys(coordinates.iter().map(|c| c.0).collect());
        // 21st Capital's keys are never offered for the client's own slots.
        keys.extend(self.our_keys.iter().copied());
        PathData {
            coordinates,
            keys,
            token_kind,
            ours,
            manual_only,
        }
    }
}

impl DefineDescriptor {
    /// Liana's editor update; `update` adds the Build your own sync.
    fn update_editor(&mut self, hws: &mut HardwareWallets, message: Message) -> Task<Message> {
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
                    self.custom_asked = None;
                }
            }
            Message::DefineDescriptor(message::DefineDescriptor::AddRecoveryPath) => {
                self.paths.push(Path::new_recovery_path());
            }
            Message::DefineDescriptor(message::DefineDescriptor::AddSafetyNetPath) => {
                if !self.paths.iter().any(|p| p.kind() == PathKind::SafetyNet) {
                    self.paths.push(Path::new_safety_net_path());
                    // Build your own: the Rescue Key comes from our website
                    // unless the client puts another key there (decided
                    // 2026-11-15).
                    if self.talia.is_none() && talia_keys::contract_network(self.network).is_some()
                    {
                        self.custom_rescue = true;
                    }
                }
            }
            Message::DefineDescriptor(message::DefineDescriptor::KeysEdited(coordinates, key)) => {
                // Build your own: a key put in a reserved slot ends the
                // reservation.
                if !matches!(key, key::SelectedKey::None) {
                    for (i, j) in &coordinates {
                        if Some(*j) == self.custom_cosigner_slot(*i) {
                            self.custom_cosigner.retain(|p| p != i);
                        }
                        if Some(*i) == self.custom_rescue_path() {
                            self.custom_rescue = false;
                        }
                    }
                }
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
                        // Build your own: our co-signer stays last.
                        let ours_last = self.custom_cosigner_slot(i).is_some();
                        if let Some(path) = self.paths.get_mut(i) {
                            if ours_last {
                                path.keys.insert(path.keys.len() - 1, None);
                            } else {
                                path.keys.push(None);
                            }
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
                                // Build your own: the cross on a reserved slot
                                // gives it back.
                                if Some(j) == self.custom_cosigner_slot(i) {
                                    self.custom_cosigner.retain(|&p| p != i);
                                }
                                if Some(i) == self.custom_rescue_path() {
                                    self.custom_rescue = false;
                                }
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
                                    // Build your own: our co-signer stays on
                                    // the layers it was given.
                                    self.custom_cosigner.retain(|&p| p != i);
                                    for p in &mut self.custom_cosigner {
                                        if *p > i {
                                            *p -= 1;
                                        }
                                    }
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
        let task = self.update_editor(hws, message);
        // Build your own: our keys follow what is reserved.
        self.sync_custom_ours(hws);
        // The 2 of 2's expanding backup holds the primary path's keys.
        if let Some(shape) = &self.talia {
            if shape.sync_backup_keys(&mut self.paths) {
                self.check_setup();
            }
        }
        task
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
                    use_taproot: self.use_taproot,
                    editing_descriptor_type: self.editing_descriptor_type,
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
                view::editor::template::custom::Ours {
                    cosigner: self
                        .custom_cosigner_paths()
                        .iter()
                        .filter_map(|&i| Some((i, self.custom_cosigner_slot(i)?)))
                        .collect(),
                    rescue: self.custom_rescue_path().is_some(),
                    filled: self.custom_ours_filled(),
                    flow: &self.keys_flow,
                    keys_available: talia_keys::contract_network(self.network).is_some(),
                },
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
            url: "https://21stcapital.com/desktop/keys".to_string(),
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
                step.talia.as_ref().unwrap().vault_shape(),
                talia_keys::VaultShape {
                    config: talia_keys::VaultConfig::Multi2of3,
                    cosigner: true,
                    rescue: true,
                    heirs: vec![9],
                    backup: None,
                    layout: None,
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
        sandbox
            .update(talia_message(TaliaMessage::Backup(false)))
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
    async fn equal_waits_compile_with_the_rescue_key_at_65535() {
        let (mut ctx, sandbox) = talia_sandbox(DescriptorTemplate::TwoOfThree);
        sandbox.load(&ctx).await;
        // A 15-month heir next to the Rescue Key, and two 12-month heirs.
        for _ in 0..3 {
            sandbox.update(talia_message(TaliaMessage::AddHeir)).await;
        }
        for (i, months) in [(0, 15u8), (1, 12), (2, 12)] {
            sandbox
                .update(talia_message(TaliaMessage::HeirMonths(i, months)))
                .await;
        }
        sandbox
            .update(talia_message(TaliaMessage::Rescue(true)))
            .await;
        let heirs = [
            test_key(3, "Heir 1"),
            test_key(6, "Heir 2"),
            test_key(7, "Heir 3"),
        ];
        sandbox.update(set_key(0, 0, &test_key(1, "Mine 1"))).await;
        sandbox.update(set_key(0, 1, &test_key(2, "Mine 2"))).await;
        for (i, heir) in heirs.iter().enumerate() {
            sandbox.update(set_key(1 + i, 0, heir)).await;
        }
        let (cosigner, rescue) = (test_key(4, "co"), test_key(5, "rescue"));
        sandbox.check(|step| {
            let waits: Vec<u16> = step.paths.iter().map(|p| p.sequence.as_u16()).collect();
            assert_eq!(waits, vec![0, 65_534, 52_596, 52_595, 65_535]);
            assert!(step.paths.iter().all(|p| p.warning.is_none()));
            // The website is asked for the months the client chose.
            assert_eq!(
                step.talia.as_ref().unwrap().vault_shape().heirs,
                vec![15, 12, 12]
            );
            waiting(step, 1);
        });
        sandbox
            .update(fetched(Some(&cosigner), Some(&rescue)))
            .await;
        sandbox.check(|step| assert!(step.valid()));

        assert!(run_apply(&sandbox, &mut ctx).await);
        let policy = ctx.descriptor.as_ref().unwrap().policy();
        let paths = policy.recovery_paths();
        assert_eq!(
            paths.keys().copied().collect::<Vec<_>>(),
            vec![52_595, 52_596, 65_534, 65_535]
        );
        let opens_at = |wait: u16, key: &Key| {
            paths[&wait]
                .thresh_origins()
                .1
                .contains_key(&key.fingerprint)
        };
        assert!(opens_at(65_535, &rescue));
        assert!(opens_at(65_534, &heirs[0]));
        assert!(opens_at(52_596, &heirs[1]));
        assert!(opens_at(52_595, &heirs[2]));
    }

    fn add_key(path: usize) -> Message {
        Message::DefineDescriptor(message::DefineDescriptor::Path(
            path,
            message::DefinePath::AddKey,
        ))
    }

    fn add_rescue_path() -> Message {
        Message::DefineDescriptor(message::DefineDescriptor::AddSafetyNetPath)
    }

    fn our_key(path: usize, index: usize) -> Message {
        talia_message(TaliaMessage::OurKey(path, index))
    }

    fn delete_key(path: usize, index: usize) -> Message {
        Message::DefineDescriptor(message::DefineDescriptor::Path(
            path,
            message::DefinePath::Key(index, message::DefineKey::Delete),
        ))
    }

    fn threshold(path: usize, t: usize) -> Message {
        Message::DefineDescriptor(message::DefineDescriptor::Path(
            path,
            message::DefinePath::ThresholdEdited(t),
        ))
    }

    /// What "Get my 21st Capital keys" leaves behind while the website is
    /// open, without opening it.
    fn ask_website(step: &mut DefineDescriptor) -> talia_keys::VaultShape {
        let shape = step.custom_shape().expect("a slot is reserved");
        waiting(step, 1);
        step.custom_asked = Some(shape.clone());
        shape
    }

    async fn build_your_own(network: Network) -> (Context, Sandbox<DefineDescriptor>) {
        let mut ctx = Context::new(
            network,
            LianaDirectory::new(PathBuf::from_str("/").unwrap()),
            crate::installer::context::RemoteBackend::None,
        );
        ctx.descriptor_template = DescriptorTemplate::Custom;
        let sandbox = Sandbox::new(DefineDescriptor::new(
            network,
            Arc::new(Mutex::new(Signer::generate(network).unwrap())),
        ));
        sandbox.load(&ctx).await;
        (ctx, sandbox)
    }

    #[tokio::test]
    async fn build_your_own_offers_our_keys_and_never_a_token() {
        let (_ctx, sandbox) = build_your_own(Network::Testnet4).await;
        sandbox.check(|step| {
            // One key on the primary path: no room for a co-signer.
            let data = step.path_data(PathKind::Primary, vec![(0, 0)]);
            assert!(data.token_kind.is_empty());
            assert_eq!(data.ours, None);
            let data = step.path_data(PathKind::Recovery, vec![(1, 0)]);
            assert!(data.token_kind.is_empty());
            assert_eq!(data.ours, None);
            assert!(!data.manual_only);
        });
        sandbox.update(add_key(0)).await;
        sandbox.update(add_rescue_path()).await;
        sandbox.check(|step| {
            let data = step.path_data(PathKind::Primary, vec![(0, 1)]);
            assert_eq!(data.ours, Some(key::OurKeyOffer::Cosigner));
            assert!(data.token_kind.is_empty());
            assert!(!data.manual_only);
            assert_eq!(step.paths[2].kind(), PathKind::SafetyNet);
            assert_eq!(step.paths[2].sequence.as_u16(), 65_535);
            // "Add Rescue Key" reserves its slot for ours at once; Edit on it
            // offers a pasted or imported key instead.
            assert_eq!(step.custom_rescue_path(), Some(2));
            let data = step.path_data(PathKind::SafetyNet, vec![(2, 0)]);
            assert_eq!(data.ours, None);
            assert!(data.token_kind.is_empty());
            assert!(data.manual_only);
        });
        // Once reserved, neither is offered a second time.
        sandbox.update(our_key(0, 0)).await;
        sandbox.update(our_key(2, 0)).await;
        sandbox.check(|step| {
            assert_eq!(step.path_data(PathKind::Primary, vec![(0, 0)]).ours, None);
            assert_eq!(step.path_data(PathKind::SafetyNet, vec![(2, 0)]).ours, None);
        });

        // We issue no keys on networks the website does not serve.
        let (_ctx, sandbox) = build_your_own(Network::Regtest).await;
        sandbox.update(add_key(0)).await;
        sandbox.update(add_rescue_path()).await;
        sandbox.check(|step| {
            assert_eq!(step.path_data(PathKind::Primary, vec![(0, 1)]).ours, None);
            assert_eq!(step.path_data(PathKind::SafetyNet, vec![(2, 0)]).ours, None);
            assert_eq!(step.custom_rescue_path(), None);
        });
    }

    #[tokio::test]
    async fn a_pasted_key_replaces_the_reserved_rescue_key() {
        let (_ctx, sandbox) = build_your_own(Network::Testnet4).await;
        let mine = test_key(1, "Mine");
        let pasted = test_key(5, "Rescue Key, pasted");
        let (cosigner, rescue) = (
            test_key(4, "21st Capital co-signer"),
            test_key(7, "21st Capital Rescue Key"),
        );
        sandbox.update(add_key(0)).await;
        sandbox.update(set_key(0, 0, &mine)).await;
        sandbox.update(our_key(0, 1)).await;
        sandbox.update(add_rescue_path()).await;
        sandbox.check(|step| {
            let shape = ask_website(step);
            assert!(shape.cosigner && shape.rescue);
        });
        sandbox
            .update(fetched(Some(&cosigner), Some(&rescue)))
            .await;
        // Edit on the Rescue Key's slot, then a pasted key.
        sandbox.update(set_key(2, 0, &pasted)).await;
        sandbox.check(|step| {
            assert_eq!(step.custom_rescue_path(), None);
            assert_eq!(step.paths[2].keys[0].as_ref(), Some(&pasted));
            // Our co-signer came as a pair with our Rescue Key: it goes too,
            // and the next press fetches the co-signer alone.
            assert!(step.paths[0].keys[1].is_none());
            assert!(step.our_keys.is_empty());
            let shape = step.custom_shape().unwrap();
            assert!(shape.cosigner && !shape.rescue);
            // Edit on the pasted key offers ours again.
            let data = step.path_data(PathKind::SafetyNet, vec![(2, 0)]);
            assert_eq!(data.ours, Some(key::OurKeyOffer::Rescue));
        });
    }

    #[test]
    fn the_rescue_key_path_takes_a_public_key_only() {
        use crate::installer::KeySourceKind;
        use liana_connect::keys::api::KeyKind;
        let rescue = PathKind::SafetyNet;
        assert!(rescue.can_choose_key_source_kind(&KeySourceKind::Manual));
        assert!(!rescue.can_choose_key_source_kind(&KeySourceKind::HotSigner));
        assert!(!rescue.can_choose_key_source_kind(&KeySourceKind::Device));
        assert!(!rescue.can_choose_key_source_kind(&KeySourceKind::Token(KeyKind::SafetyNet)));
        assert!(!rescue.can_choose_key_source_kind(&KeySourceKind::Token(KeyKind::Cosigner)));
        // The other paths are as Liana has them.
        assert!(PathKind::Primary.can_choose_key_source_kind(&KeySourceKind::Device));
        assert!(PathKind::Recovery.can_choose_key_source_kind(&KeySourceKind::HotSigner));
    }

    #[tokio::test]
    async fn picking_our_keys_reserves_their_slots_and_one_press_fetches_both() {
        let (mut ctx, sandbox) = build_your_own(Network::Testnet4).await;
        let (mine, own_recovery) = (test_key(1, "Mine"), test_key(2, "My recovery"));
        let (cosigner, rescue) = (
            test_key(4, "21st Capital co-signer"),
            test_key(5, "21st Capital Rescue Key"),
        );
        sandbox.update(add_key(0)).await;
        sandbox.update(add_rescue_path()).await;
        sandbox.update(set_key(0, 1, &mine)).await;
        sandbox.update(set_key(1, 0, &own_recovery)).await;
        // Ours is picked for the first slot: it is reserved, nothing fetched.
        sandbox.update(our_key(0, 0)).await;
        sandbox.check(|step| {
            assert!(!step.keys_flow.is_waiting());
            assert_eq!(step.custom_cosigner_slot(0), Some(1));
            // Ours sits last: the client's key moved first.
            assert_eq!(step.paths[0].keys[0].as_ref(), Some(&mine));
            assert!(step.paths[0].keys[1].is_none());
            assert!(!step.valid());
        });
        sandbox.update(our_key(2, 0)).await;
        sandbox.check(|step| {
            assert!(!step.keys_flow.is_waiting());
            // The client's own recovery layer makes it a custom vault, sent
            // layer by layer.
            let shape = ask_website(step);
            assert_eq!(shape.config, talia_keys::VaultConfig::Custom);
            assert!(shape.cosigner && shape.rescue);
            assert_eq!(
                shape.layout.unwrap().encode(),
                "0:2:k1.c;52596:1:k2;65535:1:r"
            );
        });
        sandbox
            .update(fetched(Some(&cosigner), Some(&rescue)))
            .await;
        sandbox.check(|step| {
            assert!(matches!(step.keys_flow, KeysFlow::Done));
            assert_eq!(step.paths[0].keys[1].as_ref(), Some(&cosigner));
            assert_eq!(step.paths[2].keys[0].as_ref(), Some(&rescue));
            assert_eq!(
                step.our_keys,
                vec![cosigner.fingerprint, rescue.fingerprint]
            );
            assert!(step.valid());
        });
        // Liana composes the vault from these keys as from any others.
        assert!(run_apply(&sandbox, &mut ctx).await);
    }

    #[tokio::test]
    async fn adding_the_rescue_key_later_asks_for_a_new_pair() {
        let (_ctx, sandbox) = build_your_own(Network::Testnet4).await;
        let (mine, own_recovery) = (test_key(1, "Mine"), test_key(2, "My recovery"));
        let (cosigner, cosigner2, rescue2) = (
            test_key(4, "21st Capital co-signer"),
            test_key(6, "21st Capital co-signer"),
            test_key(7, "21st Capital Rescue Key"),
        );
        sandbox.update(add_key(0)).await;
        sandbox.update(set_key(0, 0, &mine)).await;
        sandbox.update(set_key(1, 0, &own_recovery)).await;
        sandbox.update(our_key(0, 1)).await;
        sandbox.check(|step| {
            let shape = ask_website(step);
            assert!(shape.cosigner && !shape.rescue);
        });
        sandbox.update(fetched(Some(&cosigner), None)).await;
        sandbox.update(add_rescue_path()).await;
        sandbox.update(our_key(2, 0)).await;
        sandbox.check(|step| {
            // The co-signer belongs to a vault without our Rescue Key: it goes.
            assert!(step.paths[0].keys[1].is_none());
            assert!(step.our_keys.is_empty());
            assert!(!step.keys.contains_key(&cosigner.fingerprint));
            assert!(matches!(step.keys_flow, KeysFlow::Idle));
            let shape = ask_website(step);
            assert!(shape.cosigner && shape.rescue);
        });
        sandbox
            .update(fetched(Some(&cosigner2), Some(&rescue2)))
            .await;
        sandbox.check(|step| {
            assert!(matches!(step.keys_flow, KeysFlow::Done));
            assert_eq!(step.paths[0].keys[1].as_ref(), Some(&cosigner2));
            assert_eq!(step.paths[2].keys[0].as_ref(), Some(&rescue2));
        });
    }

    #[tokio::test]
    async fn a_pasted_rescue_key_stays_and_a_single_key_vault_asks_for_the_rescue_key_only() {
        let (_ctx, sandbox) = build_your_own(Network::Testnet4).await;
        let (mine, pasted) = (test_key(1, "Mine"), test_key(5, "Rescue Key, pasted"));
        sandbox.update(add_key(0)).await;
        sandbox.update(add_rescue_path()).await;
        sandbox.update(set_key(0, 0, &mine)).await;
        sandbox.update(set_key(2, 0, &pasted)).await;
        sandbox.update(our_key(0, 1)).await;
        sandbox.check(|step| {
            let shape = step.custom_shape().unwrap();
            assert!(shape.cosigner && !shape.rescue);
            assert_eq!(step.paths[2].keys[0].as_ref(), Some(&pasted));
        });

        let (_ctx, sandbox) = build_your_own(Network::Testnet4).await;
        sandbox.update(add_rescue_path()).await;
        // Without Liana's first recovery path the vault is one of ours: a
        // single key with the Rescue Key.
        sandbox.update(delete_key(1, 0)).await;
        sandbox.check(|step| {
            assert_eq!(step.paths.len(), 2);
            assert_eq!(step.custom_rescue_path(), Some(1));
        });
        sandbox.check(|step| {
            assert_eq!(
                step.custom_shape(),
                Some(talia_keys::VaultShape {
                    config: talia_keys::VaultConfig::Single,
                    cosigner: false,
                    rescue: true,
                    heirs: vec![],
                    backup: None,
                    layout: None,
                })
            );
        });
    }

    #[tokio::test]
    async fn our_cosigner_stays_last_and_its_cross_gives_the_slot_back() {
        let (_ctx, sandbox) = build_your_own(Network::Testnet4).await;
        sandbox.update(add_key(0)).await;
        sandbox.update(our_key(0, 0)).await;
        sandbox.update(add_key(0)).await;
        sandbox.check(|step| {
            assert_eq!(step.paths[0].keys.len(), 3);
            assert_eq!(step.custom_cosigner_slot(0), Some(2));
        });
        sandbox.update(delete_key(0, 2)).await;
        sandbox.check(|step| {
            assert_eq!(step.paths[0].keys.len(), 2);
            assert_eq!(step.custom_cosigner_slot(0), None);
            assert_eq!(step.custom_shape(), None);
        });
    }

    #[tokio::test]
    async fn a_changed_vault_drops_our_keys_or_refuses_a_late_answer() {
        let (_ctx, sandbox) = build_your_own(Network::Testnet4).await;
        let (mine, mine2) = (test_key(1, "Mine"), test_key(2, "Mine 2"));
        let (cosigner, cosigner2, rescue, rescue2) = (
            test_key(4, "21st Capital co-signer"),
            test_key(6, "21st Capital co-signer"),
            test_key(7, "21st Capital Rescue Key"),
            test_key(8, "21st Capital Rescue Key"),
        );
        sandbox.update(add_key(0)).await;
        sandbox.update(add_key(0)).await;
        sandbox.update(set_key(0, 0, &mine)).await;
        sandbox.update(set_key(0, 1, &mine2)).await;
        // The primary path and our Rescue Key only, no layer of the client's.
        sandbox.update(add_rescue_path()).await;
        sandbox.update(delete_key(1, 0)).await;
        sandbox.update(our_key(0, 2)).await;
        sandbox.check(|step| {
            // 3 of 3 is none of the website's types.
            assert_eq!(ask_website(step).config, talia_keys::VaultConfig::Custom);
        });
        // 2 of 3 instead of 3 of 3 while the website is open: refused.
        sandbox.update(threshold(0, 2)).await;
        sandbox
            .update(fetched(Some(&cosigner), Some(&rescue)))
            .await;
        sandbox.check(|step| {
            assert!(matches!(step.keys_flow, KeysFlow::Failed(_)));
            assert!(step.paths[0].keys[2].is_none());
            assert!(step.our_keys.is_empty());
        });
        // Fetched for the 2 of 3; back to 3 of 3, our keys leave their slots.
        sandbox.check(|step| {
            assert_eq!(ask_website(step).config, talia_keys::VaultConfig::Multi2of3);
        });
        sandbox
            .update(fetched(Some(&cosigner2), Some(&rescue2)))
            .await;
        sandbox.check(|step| assert_eq!(step.paths[0].keys[2].as_ref(), Some(&cosigner2)));
        sandbox.update(threshold(0, 3)).await;
        sandbox.check(|step| {
            assert!(step.paths[0].keys[2].is_none());
            assert!(step.our_keys.is_empty());
            assert!(matches!(step.keys_flow, KeysFlow::Idle));
            assert_eq!(step.custom_cosigner_slot(0), Some(2));
        });
    }

    #[tokio::test]
    async fn a_custom_layer_edited_later_keeps_our_keys() {
        // The layout is a record of the ask: editing a later layer does not
        // fetch again (each fetch takes another vault slot).
        let (_ctx, sandbox) = build_your_own(Network::Testnet4).await;
        let (mine, own_recovery) = (test_key(1, "Mine"), test_key(2, "My recovery"));
        let cosigner = test_key(4, "21st Capital co-signer");
        sandbox.update(add_key(0)).await;
        sandbox.update(set_key(0, 0, &mine)).await;
        sandbox.update(our_key(0, 1)).await;
        sandbox.check(|step| {
            let shape = ask_website(step);
            assert_eq!(shape.config, talia_keys::VaultConfig::Custom);
            assert_eq!(shape.layout.unwrap().encode(), "0:2:k1.c;52596:1:k2");
        });
        sandbox.update(fetched(Some(&cosigner), None)).await;
        sandbox.update(set_key(1, 0, &own_recovery)).await;
        sandbox.update(sequence(1, 30_000)).await;
        sandbox.check(|step| {
            assert!(matches!(step.keys_flow, KeysFlow::Done));
            assert_eq!(step.paths[0].keys[1].as_ref(), Some(&cosigner));
        });
    }

    fn sequence(path: usize, blocks: u16) -> Message {
        Message::DefineDescriptor(message::DefineDescriptor::Path(
            path,
            message::DefinePath::SequenceEdited(blocks),
        ))
    }

    fn add_recovery_path() -> Message {
        Message::DefineDescriptor(message::DefineDescriptor::AddRecoveryPath)
    }

    #[tokio::test]
    async fn our_cosigner_on_a_later_layer_makes_an_expanding_multisig() {
        // The client's own 2 of 2, which after a year becomes a 2 of 3 with
        // our co-signer.
        let (mut ctx, sandbox) = build_your_own(Network::Testnet4).await;
        let (a, b) = (test_key(1, "Mine A"), test_key(2, "Mine B"));
        let cosigner = test_key(4, "21st Capital co-signer");
        sandbox.update(add_key(0)).await;
        sandbox.update(set_key(0, 0, &a)).await;
        sandbox.update(set_key(0, 1, &b)).await;
        sandbox.update(add_key(1)).await;
        sandbox.update(add_key(1)).await;
        sandbox.update(threshold(1, 2)).await;
        sandbox.update(set_key(1, 0, &a)).await;
        sandbox.update(set_key(1, 1, &b)).await;
        sandbox.check(|step| {
            let data = step.path_data(PathKind::Recovery, vec![(1, 2)]);
            assert_eq!(data.ours, Some(key::OurKeyOffer::Cosigner));
        });
        sandbox.update(our_key(1, 2)).await;
        sandbox.check(|step| {
            assert_eq!(step.custom_cosigner_slot(1), Some(2));
            assert_eq!(step.custom_cosigner_slot(0), None);
            assert!(step.paths[1].keys[2].is_none());
            // Once per layer.
            assert_eq!(step.path_data(PathKind::Recovery, vec![(1, 0)]).ours, None);
            // Built by hand, it is a custom vault: the website gets it layer
            // by layer, the client's two keys numbered once.
            let shape = ask_website(step);
            assert_eq!(shape.config, talia_keys::VaultConfig::Custom);
            assert!(shape.cosigner && !shape.rescue);
            assert_eq!(shape.layout.unwrap().encode(), "0:2:k1.k2;52596:2:k1.k2.c");
        });
        sandbox.update(fetched(Some(&cosigner), None)).await;
        sandbox.check(|step| {
            assert!(matches!(step.keys_flow, KeysFlow::Done));
            assert_eq!(step.paths[1].keys[2].as_ref(), Some(&cosigner));
            assert!(step.valid());
        });
        assert!(run_apply(&sandbox, &mut ctx).await);
        let policy = ctx.descriptor.as_ref().unwrap().policy();
        let primary = policy.primary_path().thresh_origins();
        assert_eq!(primary.0, 2);
        assert!(!primary.1.contains_key(&cosigner.fingerprint));
        let later = policy.recovery_paths()[&52_596].thresh_origins();
        assert_eq!(later.0, 2);
        assert!(later.1.contains_key(&cosigner.fingerprint));
        assert!(later.1.contains_key(&a.fingerprint));
    }

    #[tokio::test]
    async fn one_cosigner_key_on_every_layer_it_is_given() {
        let (mut ctx, sandbox) = build_your_own(Network::Testnet4).await;
        let (mine, own_recovery) = (test_key(1, "Mine"), test_key(2, "My recovery"));
        let (cosigner, rescue) = (
            test_key(4, "21st Capital co-signer"),
            test_key(5, "21st Capital Rescue Key"),
        );
        sandbox.update(add_key(0)).await;
        sandbox.update(set_key(0, 0, &mine)).await;
        sandbox.update(add_key(1)).await;
        sandbox.update(set_key(1, 0, &own_recovery)).await;
        sandbox.update(add_rescue_path()).await;
        sandbox.update(our_key(0, 1)).await;
        sandbox.update(our_key(1, 1)).await;
        sandbox.check(|step| {
            // Never on the Rescue Key's path.
            let data = step.path_data(PathKind::SafetyNet, vec![(2, 0)]);
            assert_ne!(data.ours, Some(key::OurKeyOffer::Cosigner));
            let shape = ask_website(step);
            assert!(shape.cosigner && shape.rescue);
        });
        // One answer, one co-signer key, on both layers.
        sandbox
            .update(fetched(Some(&cosigner), Some(&rescue)))
            .await;
        sandbox.check(|step| {
            assert_eq!(step.paths[0].keys[1].as_ref(), Some(&cosigner));
            assert_eq!(step.paths[1].keys[1].as_ref(), Some(&cosigner));
            assert_eq!(step.paths[2].keys[0].as_ref(), Some(&rescue));
            assert_eq!(
                step.our_keys,
                vec![cosigner.fingerprint, rescue.fingerprint]
            );
        });
        // A layer given our co-signer later takes the same key, no new fetch.
        sandbox.update(add_recovery_path()).await;
        sandbox.update(sequence(3, 30_000)).await;
        sandbox.update(add_key(3)).await;
        sandbox.update(set_key(3, 0, &mine)).await;
        sandbox.update(our_key(3, 1)).await;
        sandbox.check(|step| {
            assert_eq!(step.paths[3].keys[1].as_ref(), Some(&cosigner));
            assert!(matches!(step.keys_flow, KeysFlow::Done));
            assert!(step.custom_ours_filled());
            assert!(step.valid());
        });
        // Liana gives each use of the key its own derivation step.
        assert!(run_apply(&sandbox, &mut ctx).await);
        let policy = ctx.descriptor.as_ref().unwrap().policy();
        assert!(policy
            .primary_path()
            .thresh_origins()
            .1
            .contains_key(&cosigner.fingerprint));
        for wait in [30_000, 52_596] {
            assert!(policy.recovery_paths()[&wait]
                .thresh_origins()
                .1
                .contains_key(&cosigner.fingerprint));
        }

        // Our co-signer never signs alone.
        sandbox.update(threshold(1, 1)).await;
        sandbox.check(|step| {
            assert_eq!(step.paths[1].warning, Some(PathWarning::OurCosignerAlone));
            assert!(!step.valid());
        });
        sandbox.update(threshold(1, 2)).await;
        sandbox.check(|step| {
            assert_eq!(step.paths[1].warning, None);
            assert!(step.valid());
        });

        // The cross on one layer's co-signer leaves the others as they are.
        sandbox.update(delete_key(3, 1)).await;
        sandbox.check(|step| {
            assert_eq!(step.paths[3].keys, vec![Some(mine.clone())]);
            assert_eq!(step.custom_cosigner_slot(3), None);
            assert_eq!(step.paths[0].keys[1].as_ref(), Some(&cosigner));
            assert_eq!(step.paths[1].keys[1].as_ref(), Some(&cosigner));
            assert!(matches!(step.keys_flow, KeysFlow::Done));
        });
        // A layer left with our co-signer only gives it up.
        sandbox.update(delete_key(1, 0)).await;
        sandbox.check(|step| {
            assert_eq!(step.paths[1].keys, vec![None]);
            assert_eq!(step.custom_cosigner_slot(1), None);
            assert_eq!(step.paths[0].keys[1].as_ref(), Some(&cosigner));
        });
    }

    #[tokio::test]
    async fn our_cosigner_follows_its_layer_when_an_earlier_one_goes() {
        let (_ctx, sandbox) = build_your_own(Network::Testnet4).await;
        let (mine, own_recovery) = (test_key(1, "Mine"), test_key(2, "My recovery"));
        sandbox.update(set_key(1, 0, &own_recovery)).await;
        sandbox.update(add_recovery_path()).await;
        sandbox.update(sequence(2, 30_000)).await;
        sandbox.update(add_key(2)).await;
        sandbox.update(set_key(2, 0, &mine)).await;
        sandbox.update(our_key(2, 1)).await;
        sandbox.check(|step| assert_eq!(step.custom_cosigner_slot(2), Some(1)));
        // The first recovery layer goes with its only key.
        sandbox.update(delete_key(1, 0)).await;
        sandbox.check(|step| {
            assert_eq!(step.paths.len(), 2);
            assert_eq!(step.paths[1].sequence.as_u16(), 30_000);
            assert_eq!(step.custom_cosigner_slot(1), Some(1));
            assert_eq!(step.custom_cosigner_slot(2), None);
        });
    }

    #[tokio::test]
    async fn a_two_of_two_that_expands_takes_our_cosigner_as_its_third_key() {
        // The client's own 2 of 2, which after 6 months becomes a 2 of 3
        // with our co-signer: the website configurator's expanding multisig.
        let (mut ctx, sandbox) = talia_sandbox(DescriptorTemplate::TwoOfTwo);
        sandbox.load(&ctx).await;
        let (a, b) = (test_key(1, "Mine A"), test_key(2, "Mine B"));
        let cosigner = test_key(4, "21st Capital co-signer");
        for msg in [
            TaliaMessage::CosignerOurs(false),
            TaliaMessage::Backup(true),
            TaliaMessage::BackupKind(talia::BackupKind::TwoOfThree),
            TaliaMessage::BackupHolderOurs(true),
            TaliaMessage::Rescue(false),
        ] {
            sandbox.update(talia_message(msg)).await;
        }
        sandbox.update(set_key(0, 0, &a)).await;
        sandbox.update(set_key(0, 1, &b)).await;
        sandbox.check(|step| {
            // The backup way in holds the primary path's two keys as they are.
            assert_eq!(step.paths[1].keys[0].as_ref(), Some(&a));
            assert_eq!(step.paths[1].keys[1].as_ref(), Some(&b));
            assert!(step.paths[1].keys[2].is_none());
            let asked = step.talia.as_ref().unwrap().vault_shape();
            assert_eq!(asked.config, talia_keys::VaultConfig::Multi2of2);
            assert!(!asked.cosigner && !asked.rescue);
            assert_eq!(
                asked.backup,
                Some(talia_keys::Backup {
                    kind: talia_keys::BackupKind::ExpandMultisig,
                    holder_21c: true,
                    months: 6,
                })
            );
            waiting(step, 1);
        });
        sandbox.update(fetched(Some(&cosigner), None)).await;
        sandbox.check(|step| {
            assert!(matches!(step.keys_flow, KeysFlow::Done));
            assert_eq!(step.paths[1].keys[2].as_ref(), Some(&cosigner));
            assert!(step.talia_locked());
            assert!(step.valid());
        });
        assert!(run_apply(&sandbox, &mut ctx).await);
        let policy = ctx.descriptor.as_ref().unwrap().policy();
        let primary = policy.primary_path().thresh_origins();
        assert_eq!(primary.0, 2);
        assert!(!primary.1.contains_key(&cosigner.fingerprint));
        let backup = policy.recovery_paths()[&26_298].thresh_origins();
        assert_eq!(backup.0, 2);
        for key in [&a, &b, &cosigner] {
            assert!(backup.1.contains_key(&key.fingerprint));
        }

        // "Change" takes our key out of the backup way in too.
        sandbox
            .update(talia_message(TaliaMessage::ChangeShape))
            .await;
        sandbox.check(|step| {
            assert!(step.paths[1].keys[2].is_none());
            assert!(step.our_keys.is_empty());
            assert!(!step.talia_locked());
        });
    }
}

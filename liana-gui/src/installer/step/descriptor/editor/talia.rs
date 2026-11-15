//! The three vault shapes of the iPhone app, built on Liana's own paths: a
//! primary path, the 2 of 2's backup way in, one recovery path per heir and
//! one for the 21st Capital Rescue Key. Liana composes the descriptor (Taproot or SegWit) and orders the recovery
//! paths by their wait.
//!
//! Liana refuses two paths opening at the same block, while the iPhone app
//! allows a 15-month heir next to the Rescue Key and heirs of equal months
//! (decided 2026-11-14). So the months become blocks here: the Rescue Key always
//! opens at exactly 65535 blocks, and an heir whose wait would equal another
//! path's opens one block earlier per collision, the later-listed heir giving
//! way. The client still sees the months everywhere.

use std::collections::HashSet;

use std::ops::RangeInclusive;

use crate::installer::{
    context::DescriptorTemplate,
    descriptor::{Path, PathSequence},
    talia_keys::{self, VaultConfig, VaultShape},
};

/// Liana's calendar-average month (30.4375 days of blocks), as the iPhone app
/// counts it.
pub const BLOCKS_PER_MONTH: u32 = 4383;
/// BIP-68 relative locks are 16-bit in blocks.
pub const MAX_OLDER_BLOCKS: u32 = 65535;
/// The Rescue Key's wait.
pub const RESCUE_MONTHS: u8 = 15;
/// An heir's wait.
pub const HEIR_MONTHS: RangeInclusive<u8> = 8..=15;
/// A new heir's wait, else the nearest free one.
pub const DEFAULT_HEIR_MONTHS: u8 = 12;
pub const MAX_HEIRS: usize = 5;
/// The 2 of 2's backup way in opens after 3 to 10 months, always before the
/// heirs, as on the website's configurator (decided 2026-11-15).
pub const BACKUP_MONTHS: RangeInclusive<u8> = 3..=10;
pub const DEFAULT_BACKUP_MONTHS: u8 = 6;

/// Months to blocks on Liana's calendar-average scale, capped at the BIP-68
/// maximum (15 months is the cap itself).
pub fn months_to_blocks(months: u8) -> u16 {
    (months as u32 * BLOCKS_PER_MONTH).min(MAX_OLDER_BLOCKS) as u16
}

/// A shape of the iPhone app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaliaTemplate {
    TwoOfThree,
    TwoOfTwo,
    SingleWithRescue,
}

impl TaliaTemplate {
    pub fn from_template(template: DescriptorTemplate) -> Option<Self> {
        match template {
            DescriptorTemplate::TwoOfThree => Some(Self::TwoOfThree),
            DescriptorTemplate::TwoOfTwo => Some(Self::TwoOfTwo),
            DescriptorTemplate::SingleWithRescue => Some(Self::SingleWithRescue),
            DescriptorTemplate::Custom => None,
        }
    }

    /// The keys the client holds on the primary path.
    pub fn own_keys(self) -> usize {
        match self {
            Self::TwoOfThree => 2,
            Self::TwoOfTwo | Self::SingleWithRescue => 1,
        }
    }

    pub fn threshold(self) -> usize {
        match self {
            Self::TwoOfThree | Self::TwoOfTwo => 2,
            Self::SingleWithRescue => 1,
        }
    }

    /// Whether the primary path has a slot that 21st Capital or the client holds.
    pub fn has_cosigner_slot(self) -> bool {
        self.threshold() > 1
    }

    pub fn config(self) -> VaultConfig {
        match self {
            Self::TwoOfThree => VaultConfig::Multi2of3,
            Self::TwoOfTwo => VaultConfig::Multi2of2,
            Self::SingleWithRescue => VaultConfig::Single,
        }
    }
}

/// What the 2 of 2's backup way in is once its wait has passed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackupKind {
    /// Expanding multisig: the same two keys and one added key, any two of
    /// the three. No key ever spends alone.
    TwoOfThree,
    /// One key of the client's own, spending alone.
    OneKey,
}

/// The client's choices for the 2 of 2's backup way in. Exactly one key is
/// added, never more, as on the website.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Backup {
    pub kind: Option<BackupKind>,
    /// Who holds the added key where the client may choose: 21st Capital
    /// (true) or the client.
    pub holder_21c: Option<bool>,
    pub months: u8,
}

impl Default for Backup {
    fn default() -> Self {
        Self {
            kind: None,
            holder_21c: None,
            months: DEFAULT_BACKUP_MONTHS,
        }
    }
}

/// The client's choices for a vault of a [`TaliaTemplate`]. The paths are the
/// rest of the truth: the primary path, then the 2 of 2's backup way in when
/// it has one, then the heirs, then the Rescue Key's path when the vault has
/// it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaliaShape {
    pub template: TaliaTemplate,
    /// 21st Capital holds the co-signer slot (the default), else the client.
    pub cosigner_21c: bool,
    /// The client's answer to the Rescue Key question, where it is asked.
    pub rescue_answer: Option<bool>,
    /// Whether the last path is the Rescue Key's.
    pub rescue_path: bool,
    /// The 2 of 2's answer to "What happens if you lose one of your two keys?".
    pub backup_answer: Option<bool>,
    /// The backup way in's choices, kept while the answer is No.
    pub backup: Backup,
    /// Whether `paths[1]` is the backup way in.
    pub backup_path: bool,
    /// Each heir's wait in months, in the order of their paths.
    heir_months: Vec<u8>,
}

impl TaliaShape {
    pub fn new(template: TaliaTemplate) -> Self {
        Self {
            template,
            cosigner_21c: true,
            rescue_answer: None,
            rescue_path: true,
            backup_answer: None,
            backup: Backup::default(),
            backup_path: false,
            heir_months: Vec::new(),
        }
    }

    /// The paths a new vault of this shape starts with: the primary path,
    /// the client's slots first and the co-signer's last, then the Rescue
    /// Key's path, which a vault without heirs always has.
    pub fn initial_paths(&self) -> Vec<Path> {
        let keys = self.template.own_keys() + usize::from(self.template.has_cosigner_slot());
        vec![
            Path::new_primary_path()
                .with_n_keys(keys)
                .with_threshold(self.template.threshold()),
            rescue_path(),
        ]
    }

    /// How many heirs the paths hold.
    pub fn heirs(&self, paths: &[Path]) -> usize {
        paths
            .len()
            .saturating_sub(1 + usize::from(self.backup_path) + usize::from(self.rescue_path))
    }

    /// A vault always keeps a way back: with no heir holding a key and no
    /// backup way in, the Rescue Key is it. A single-key vault always has
    /// the Rescue Key.
    pub fn rescue_required(&self, heirs: usize) -> bool {
        self.template == TaliaTemplate::SingleWithRescue || (heirs == 0 && !self.backup_path)
    }

    pub fn rescue(&self, heirs: usize) -> bool {
        self.rescue_required(heirs) || self.rescue_answer == Some(true)
    }

    /// Whether every question of the shape has its answer.
    pub fn complete(&self, heirs: usize) -> bool {
        (self.rescue_required(heirs) || self.rescue_answer.is_some()) && self.backup_complete()
    }

    /// Whether the backup question, where it is asked, has all its answers.
    pub fn backup_complete(&self) -> bool {
        if !self.asks_backup() {
            return true;
        }
        match self.backup_answer {
            None => false,
            Some(false) => true,
            Some(true) => self.backup.kind.is_some() && self.backup_holder_21c().is_some(),
        }
    }

    /// Only the 2 of 2 asks "What happens if you lose one of your two keys?".
    pub fn asks_backup(&self) -> bool {
        self.template == TaliaTemplate::TwoOfTwo
    }

    /// Whether the client cannot choose who holds the added key: a key that
    /// spends alone is always the client's (ours that does is the Rescue
    /// Key, at the very end), and with our co-signer already in the vault the
    /// third key is a device of the client's.
    pub fn backup_holder_forced(&self) -> bool {
        self.backup.kind == Some(BackupKind::OneKey) || self.cosigner_21c
    }

    /// Who holds the added key: 21st Capital (true), the client (false), or
    /// not answered yet.
    pub fn backup_holder_21c(&self) -> Option<bool> {
        if self.backup_holder_forced() {
            Some(false)
        } else {
            self.backup.holder_21c
        }
    }

    /// Adds, reshapes or drops the backup path so the paths match the
    /// answers, then the Rescue Key's path and every lock.
    pub fn sync_backup_path(&mut self, paths: &mut Vec<Path>) {
        let want = if self.asks_backup() && self.backup_answer == Some(true) {
            self.backup.kind
        } else {
            None
        };
        let slots = want.map(|kind| match kind {
            BackupKind::TwoOfThree => 3,
            BackupKind::OneKey => 1,
        });
        // A path of the right size keeps its keys.
        if self.backup_path && Some(paths[1].keys.len()) != slots {
            paths.remove(1);
            self.backup_path = false;
        }
        if let Some(slots) = slots {
            if !self.backup_path {
                let path = Path::new_recovery_path()
                    .with_n_keys(slots)
                    .with_threshold(if slots > 1 { 2 } else { 1 });
                paths.insert(1, path);
                self.backup_path = true;
            }
        }
        self.sync_backup_keys(paths);
        self.sync_rescue_path(paths);
    }

    /// Where the added key sits: (path, slot).
    pub fn backup_added_slot(&self) -> Option<(usize, usize)> {
        if !self.backup_path {
            return None;
        }
        match self.backup.kind {
            Some(BackupKind::TwoOfThree) => Some((1, 2)),
            Some(BackupKind::OneKey) => Some((1, 0)),
            None => None,
        }
    }

    /// The added key's slot when 21st Capital holds it.
    pub fn backup_21c_slot(&self) -> Option<(usize, usize)> {
        self.backup_added_slot()
            .filter(|_| self.backup_holder_21c() == Some(true))
    }

    /// The expanding 2 of 3 holds the primary path's two keys as they are:
    /// they follow the primary slots. Whether anything changed.
    pub fn sync_backup_keys(&self, paths: &mut [Path]) -> bool {
        if !self.backup_path || self.backup.kind != Some(BackupKind::TwoOfThree) {
            return false;
        }
        let mut changed = false;
        for j in 0..2 {
            let key = paths[0].keys.get(j).cloned().flatten();
            if let Some(slot) = paths[1].keys.get_mut(j) {
                if *slot != key {
                    *slot = key;
                    changed = true;
                }
            }
        }
        changed
    }

    /// Adds or drops the Rescue Key's path so the paths match the answers,
    /// then gives every heir's path its lock.
    pub fn sync_rescue_path(&mut self, paths: &mut Vec<Path>) {
        let want = self.rescue(self.heirs(paths));
        if want && !self.rescue_path {
            paths.push(rescue_path());
            self.rescue_path = true;
        } else if !want && self.rescue_path {
            paths.pop();
            self.rescue_path = false;
        }
        self.sync_heir_locks(paths);
    }

    /// Each path's months as blocks, every path at a block of its own: the
    /// Rescue Key keeps exactly 65535, the backup way in comes first, and an
    /// heir whose wait is already taken by the Rescue Key or an heir listed
    /// before opens one block earlier per collision.
    fn sync_heir_locks(&self, paths: &mut [Path]) {
        let mut taken = HashSet::new();
        if self.rescue_path {
            taken.insert(months_to_blocks(RESCUE_MONTHS));
        }
        if self.backup_path {
            let blocks = months_to_blocks(self.backup.months);
            taken.insert(blocks);
            paths[1].sequence = PathSequence::Recovery(blocks);
        }
        for (i, months) in self.heir_months.iter().enumerate() {
            let mut blocks = months_to_blocks(*months);
            while taken.contains(&blocks) {
                blocks -= 1;
            }
            taken.insert(blocks);
            if let Some(path) = paths.get_mut(self.heir_path(i)) {
                path.sequence = PathSequence::Recovery(blocks);
            }
        }
    }

    /// Index of the `i`th heir's path.
    pub fn heir_path(&self, i: usize) -> usize {
        1 + usize::from(self.backup_path) + i
    }

    /// Index of the Rescue Key's path, if the vault has it.
    pub fn rescue_path_index(&self, paths: &[Path]) -> Option<usize> {
        self.rescue_path.then(|| paths.len() - 1)
    }

    /// Index of the co-signer slot on the primary path, when 21st Capital holds it.
    pub fn cosigner_slot(&self, paths: &[Path]) -> Option<usize> {
        (self.template.has_cosigner_slot() && self.cosigner_21c).then(|| paths[0].keys.len() - 1)
    }

    /// The heirs' waits in months, in the order of their paths.
    pub fn heir_months(&self) -> &[u8] {
        &self.heir_months
    }

    /// The waits an heir may have: heirs always open after the backup way in.
    pub fn heir_months_range(&self) -> RangeInclusive<u8> {
        let floor = if self.backup_path {
            (*HEIR_MONTHS.start()).max(self.backup.months + 1)
        } else {
            *HEIR_MONTHS.start()
        };
        floor..=*HEIR_MONTHS.end()
    }

    /// A new heir's path, at the first wait no heir has yet nearest the default.
    pub fn add_heir(&mut self, paths: &mut Vec<Path>) {
        let heirs = self.heirs(paths);
        if heirs >= MAX_HEIRS {
            return;
        }
        let range = self.heir_months_range();
        let months = [12, 11, 13, 10, 14, 9, 8]
            .iter()
            .copied()
            .find(|m| range.contains(m) && !self.heir_months.contains(m))
            .unwrap_or(DEFAULT_HEIR_MONTHS);
        paths.insert(self.heir_path(heirs), Path::new_recovery_path());
        self.heir_months.push(months);
        self.sync_rescue_path(paths);
    }

    pub fn remove_heir(&mut self, paths: &mut Vec<Path>, i: usize) {
        if i >= self.heirs(paths) {
            return;
        }
        paths.remove(self.heir_path(i));
        self.heir_months.remove(i);
        if self.heirs(paths) == 0 && !self.backup_path {
            // The Rescue Key is required again: the next heir asks afresh.
            self.rescue_answer = None;
        }
        self.sync_rescue_path(paths);
    }

    pub fn set_heir_months(&mut self, paths: &mut [Path], i: usize, months: u8) {
        if i < self.heir_months.len() && self.heir_months_range().contains(&months) {
            self.heir_months[i] = months;
            self.sync_heir_locks(paths);
        }
    }

    pub fn set_rescue(&mut self, paths: &mut Vec<Path>, include: bool) {
        if !self.rescue_required(self.heirs(paths)) {
            self.rescue_answer = Some(include);
            self.sync_rescue_path(paths);
        }
    }

    /// The answer to "What happens if you lose one of your two keys?".
    pub fn set_backup(&mut self, paths: &mut Vec<Path>, include: bool) {
        if !self.asks_backup() {
            return;
        }
        self.backup_answer = Some(include);
        if !include && self.heirs(paths) == 0 {
            // The Rescue Key is the way back again: required, not asked.
            self.rescue_answer = None;
        }
        self.sync_backup_path(paths);
        self.raise_heirs(paths);
    }

    pub fn set_backup_kind(&mut self, paths: &mut Vec<Path>, kind: BackupKind) {
        if self.backup.kind != Some(kind) {
            self.backup.kind = Some(kind);
            self.sync_backup_path(paths);
        }
    }

    /// Who holds the added key; the slot changes hands, so whatever key it
    /// held goes.
    pub fn set_backup_holder(&mut self, paths: &mut [Path], ours: bool) {
        if self.backup_holder_forced() || self.backup.holder_21c == Some(ours) {
            return;
        }
        self.backup.holder_21c = Some(ours);
        if let Some((i, j)) = self.backup_added_slot() {
            paths[i].keys[j] = None;
        }
    }

    pub fn set_backup_months(&mut self, paths: &mut [Path], months: u8) {
        if BACKUP_MONTHS.contains(&months) {
            self.backup.months = months;
            self.raise_heirs(paths);
        }
    }

    /// Heirs below the floor move up to it: they always open after the
    /// backup way in.
    fn raise_heirs(&mut self, paths: &mut [Path]) {
        let floor = *self.heir_months_range().start();
        for months in &mut self.heir_months {
            if *months < floor {
                *months = floor;
            }
        }
        self.sync_heir_locks(paths);
    }

    /// What the website is asked for.
    pub fn vault_shape(&self) -> VaultShape {
        let backup = self.backup_path.then(|| talia_keys::Backup {
            kind: match self.backup.kind {
                Some(BackupKind::OneKey) => talia_keys::BackupKind::SingleSig,
                _ => talia_keys::BackupKind::ExpandMultisig,
            },
            holder_21c: self.backup_holder_21c() == Some(true),
            months: self.backup.months,
        });
        VaultShape {
            config: self.template.config(),
            cosigner: self.template.has_cosigner_slot() && self.cosigner_21c,
            rescue: self.rescue_path,
            heirs: self.heir_months.clone(),
            backup,
            layout: None,
        }
    }

    /// Whether the vault holds any 21st Capital key at all.
    pub fn uses_21c(&self, paths: &[Path]) -> bool {
        self.cosigner_slot(paths).is_some() || self.rescue_path || self.backup_21c_slot().is_some()
    }
}

/// Where "Get my 21st Capital keys" stands.
#[derive(Debug, Default)]
pub enum KeysFlow {
    #[default]
    Idle,
    /// The browser is open on the website and the loopback listener waits.
    /// Dropping the handle stops the wait and the listening.
    Waiting {
        attempt: u64,
        url: String,
        _abort: iced::task::Handle,
    },
    Failed(String),
    /// The 21st Capital keys are in their slots.
    Done,
}

impl KeysFlow {
    pub fn is_waiting(&self) -> bool {
        matches!(self, Self::Waiting { .. })
    }
}

fn rescue_path() -> Path {
    let mut path = Path::new_recovery_path();
    path.sequence = PathSequence::Recovery(months_to_blocks(RESCUE_MONTHS));
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn months_are_counted_as_the_iphone_app_counts_them() {
        assert_eq!(months_to_blocks(8), 35_064);
        assert_eq!(months_to_blocks(12), 52_596);
        assert_eq!(months_to_blocks(14), 61_362);
        // 15 months would be 65_745 blocks: capped.
        assert_eq!(months_to_blocks(15), 65_535);
    }

    fn sequences(paths: &[Path]) -> Vec<u16> {
        paths.iter().map(|p| p.sequence.as_u16()).collect()
    }

    #[test]
    fn a_two_of_three_starts_with_the_rescue_key_as_its_way_back() {
        let shape = TaliaShape::new(TaliaTemplate::TwoOfThree);
        let paths = shape.initial_paths();
        assert_eq!(paths[0].keys.len(), 3);
        assert_eq!(paths[0].threshold, 2);
        assert_eq!(sequences(&paths), vec![0, 65_535]);
        assert_eq!(shape.cosigner_slot(&paths), Some(2));
        assert_eq!(shape.rescue_path_index(&paths), Some(1));
        assert!(shape.rescue_required(0));
        assert!(shape.complete(0));
        assert_eq!(
            shape.vault_shape(),
            VaultShape {
                config: VaultConfig::Multi2of3,
                cosigner: true,
                rescue: true,
                heirs: vec![],
                backup: None,
                layout: None,
            }
        );
    }

    #[test]
    fn an_heir_makes_the_rescue_key_a_question() {
        let mut shape = TaliaShape::new(TaliaTemplate::TwoOfTwo);
        let mut paths = shape.initial_paths();
        assert_eq!(paths[0].keys.len(), 2);
        // The 2 of 2 asks for its backup way in first.
        assert!(!shape.complete(0));
        shape.set_backup(&mut paths, false);
        assert!(shape.complete(0));
        shape.add_heir(&mut paths);
        // Not required any more, not answered yet: no Rescue Key path.
        assert_eq!(shape.heirs(&paths), 1);
        assert!(!shape.complete(1));
        assert!(!shape.rescue_path);
        assert_eq!(sequences(&paths), vec![0, 52_596]);

        shape.set_rescue(&mut paths, true);
        assert!(shape.complete(1));
        assert_eq!(sequences(&paths), vec![0, 52_596, 65_535]);

        // A second heir goes before the Rescue Key's path, at a free wait.
        shape.add_heir(&mut paths);
        assert_eq!(sequences(&paths), vec![0, 52_596, 48_213, 65_535]);
        shape.set_heir_months(&mut paths, 1, 9);
        assert_eq!(shape.heir_months(), &[12, 9]);
        assert_eq!(shape.vault_shape().heirs, vec![12, 9]);

        shape.set_rescue(&mut paths, false);
        assert_eq!(sequences(&paths), vec![0, 52_596, 39_447]);

        // The last heir gone, the Rescue Key is required again.
        shape.remove_heir(&mut paths, 0);
        shape.remove_heir(&mut paths, 0);
        assert_eq!(shape.rescue_answer, None);
        assert_eq!(sequences(&paths), vec![0, 65_535]);
    }

    #[test]
    fn a_single_key_vault_always_has_the_rescue_key() {
        let mut shape = TaliaShape::new(TaliaTemplate::SingleWithRescue);
        let mut paths = shape.initial_paths();
        assert_eq!(paths[0].keys.len(), 1);
        assert_eq!(shape.cosigner_slot(&paths), None);
        shape.add_heir(&mut paths);
        shape.set_rescue(&mut paths, false);
        assert!(shape.rescue_path);
        assert!(shape.complete(1));
        assert_eq!(sequences(&paths), vec![0, 52_596, 65_535]);
        assert_eq!(shape.vault_shape().config, VaultConfig::Single);
        assert!(!shape.vault_shape().cosigner);
    }

    #[test]
    fn heirs_stop_at_five() {
        let mut shape = TaliaShape::new(TaliaTemplate::TwoOfThree);
        let mut paths = shape.initial_paths();
        for _ in 0..7 {
            shape.add_heir(&mut paths);
        }
        assert_eq!(shape.heirs(&paths), MAX_HEIRS);
        assert_eq!(shape.heir_months(), &[12, 11, 13, 10, 14]);
    }

    #[test]
    fn a_self_held_co_signer_and_no_rescue_key_uses_no_21c_key() {
        let mut shape = TaliaShape::new(TaliaTemplate::TwoOfThree);
        let mut paths = shape.initial_paths();
        shape.cosigner_21c = false;
        shape.add_heir(&mut paths);
        shape.set_rescue(&mut paths, false);
        assert!(!shape.uses_21c(&paths));
        assert_eq!(shape.cosigner_slot(&paths), None);
    }

    #[test]
    fn a_15_month_heir_gives_way_to_the_rescue_key() {
        let mut shape = TaliaShape::new(TaliaTemplate::TwoOfThree);
        let mut paths = shape.initial_paths();
        shape.add_heir(&mut paths);
        shape.set_heir_months(&mut paths, 0, 15);
        // No Rescue Key yet: the heir has 65535 to itself.
        assert_eq!(sequences(&paths), vec![0, 65_535]);
        shape.set_rescue(&mut paths, true);
        // The Rescue Key keeps exactly 65535; the heir opens a block earlier.
        assert_eq!(sequences(&paths), vec![0, 65_534, 65_535]);
        shape.add_heir(&mut paths);
        shape.set_heir_months(&mut paths, 1, 15);
        assert_eq!(sequences(&paths), vec![0, 65_534, 65_533, 65_535]);
        // The client still reads 15 months, and so does the website.
        assert_eq!(shape.heir_months(), &[15, 15]);
        assert_eq!(shape.vault_shape().heirs, vec![15, 15]);
        // The first heir gone, the other moves up to the free block.
        shape.remove_heir(&mut paths, 0);
        assert_eq!(sequences(&paths), vec![0, 65_534, 65_535]);
    }

    #[test]
    fn heirs_of_equal_months_open_a_block_apart() {
        let mut shape = TaliaShape::new(TaliaTemplate::TwoOfTwo);
        let mut paths = shape.initial_paths();
        shape.add_heir(&mut paths);
        shape.add_heir(&mut paths);
        shape.set_heir_months(&mut paths, 1, 12);
        shape.set_rescue(&mut paths, false);
        assert_eq!(sequences(&paths), vec![0, 52_596, 52_595]);
        assert_eq!(shape.heir_months(), &[12, 12]);
        shape.set_heir_months(&mut paths, 1, 9);
        assert_eq!(sequences(&paths), vec![0, 52_596, 39_447]);
    }

    #[test]
    fn the_two_of_two_backup_way_in_follows_the_website() {
        let mut shape = TaliaShape::new(TaliaTemplate::TwoOfTwo);
        let mut paths = shape.initial_paths();
        shape.set_backup(&mut paths, true);
        // Yes is not the whole answer: which kind.
        assert!(!shape.complete(0));
        assert!(!shape.backup_path);
        shape.set_backup_kind(&mut paths, BackupKind::TwoOfThree);
        assert_eq!(paths[1].keys.len(), 3);
        assert_eq!(paths[1].threshold, 2);
        // Our co-signer already in the vault: the third key is the client's.
        assert!(shape.backup_holder_forced());
        assert_eq!(shape.backup_holder_21c(), Some(false));
        // A way back exists: the Rescue Key becomes a question.
        assert!(!shape.rescue_required(0));
        assert!(!shape.complete(0));
        assert_eq!(sequences(&paths), vec![0, 26_298]);
        shape.set_rescue(&mut paths, true);
        assert!(shape.complete(0));
        assert_eq!(sequences(&paths), vec![0, 26_298, 65_535]);
        assert_eq!(
            shape.vault_shape().backup,
            Some(talia_keys::Backup {
                kind: talia_keys::BackupKind::ExpandMultisig,
                holder_21c: false,
                months: 6,
            })
        );

        // The client holds both primary keys: the added key may be ours.
        shape.cosigner_21c = false;
        assert!(!shape.backup_holder_forced());
        assert!(!shape.complete(0));
        shape.set_backup_holder(&mut paths, true);
        assert!(shape.complete(0));
        assert_eq!(shape.backup_21c_slot(), Some((1, 2)));
        assert!(shape.uses_21c(&paths));
        let asked = shape.vault_shape();
        assert!(!asked.cosigner);
        assert!(asked.needs_cosigner());

        // Heirs always open after it.
        shape.set_backup_months(&mut paths, 3);
        shape.add_heir(&mut paths);
        shape.set_heir_months(&mut paths, 0, 8);
        assert_eq!(shape.heir_months(), &[8]);
        shape.set_backup_months(&mut paths, 9);
        assert_eq!(shape.heir_months_range(), 10..=15);
        assert_eq!(shape.heir_months(), &[10]);
        shape.set_heir_months(&mut paths, 0, 9);
        assert_eq!(shape.heir_months(), &[10]);
        assert_eq!(sequences(&paths), vec![0, 39_447, 43_830, 65_535]);

        // One key alone: always the client's, one slot.
        shape.set_backup_kind(&mut paths, BackupKind::OneKey);
        assert_eq!(paths[1].keys.len(), 1);
        assert_eq!(paths[1].threshold, 1);
        assert_eq!(shape.backup_holder_21c(), Some(false));
        assert_eq!(shape.backup_21c_slot(), None);

        // No backup way in: its path goes, the heir keeps its place.
        shape.set_backup(&mut paths, false);
        assert!(!shape.backup_path);
        assert_eq!(shape.heirs(&paths), 1);
        assert_eq!(sequences(&paths), vec![0, 43_830, 65_535]);
    }

    #[test]
    fn a_kept_backup_path_keeps_its_added_key() {
        use crate::installer::descriptor::PathWarning;
        let mut shape = TaliaShape::new(TaliaTemplate::TwoOfTwo);
        let mut paths = shape.initial_paths();
        shape.set_backup(&mut paths, true);
        shape.set_backup_kind(&mut paths, BackupKind::OneKey);
        // A mark on the path: it survives only if the path is the same one.
        paths[1].warning = Some(PathWarning::DuplicateSequence);
        shape.set_backup(&mut paths, true);
        shape.set_backup_kind(&mut paths, BackupKind::OneKey);
        shape.set_backup_months(&mut paths, 8);
        assert_eq!(paths[1].warning, Some(PathWarning::DuplicateSequence));
        // Another kind is another path.
        shape.set_backup_kind(&mut paths, BackupKind::TwoOfThree);
        assert_eq!(paths[1].warning, None);
    }
}

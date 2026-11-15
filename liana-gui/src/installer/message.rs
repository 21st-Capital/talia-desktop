use liana::{
    descriptors::LianaDescriptor,
    miniscript::{
        bitcoin::{
            bip32::{ChildNumber, Fingerprint},
            Network,
        },
        DescriptorPublicKey,
    },
};
use liana_i18n::SupportedLocale;
use std::collections::HashMap;

use super::{
    context,
    step::descriptor::editor::key::{EditKeyAliasMessage, SelectKeySourceMessage, SelectedKey},
    talia_keys::{FetchedKeys, HandoffError},
    Error,
};
use crate::{
    app::{
        settings::{self, ProviderKey},
        view::Close,
    },
    backup::Backup,
    download::{DownloadError, Progress},
    export::ImportExportMessage,
    hw::HardwareWalletMessage,
    installer::{decrypt::Decrypt, descriptor::PathKind},
    node::{
        bitcoind::{Bitcoind, ConfigField, RpcAuthType},
        electrum, NodeType,
    },
    services::connect::client::auth::AuthClient,
};

#[derive(Debug, Clone)]
pub enum Message {
    UserActionDone(bool),
    Exit(Box<settings::WalletSettings>, Option<Bitcoind>),
    Clipboard(String),
    Next,
    Skip,
    Previous,
    BackToLauncher(Network),
    Install,
    Close,
    Reload,
    Select(usize),
    UseHotSigner,
    Installed(settings::WalletId, Result<settings::WalletSettings, Error>),
    CreateTaprootDescriptor(bool),
    ShowDescriptorTypeOptions(bool),
    SelectDescriptorTemplate(context::DescriptorTemplate),
    SelectBackend(SelectBackend),
    ImportRemoteWallet(ImportRemoteWallet),
    SelectBitcoindType(SelectBitcoindTypeMsg),
    InternalBitcoind(InternalBitcoindMsg),
    DefineNode(DefineNode),
    DefineDescriptor(DefineDescriptor),
    ImportXpub(Fingerprint, Result<DescriptorPublicKey, Error>),
    HardwareWallets(HardwareWalletMessage),
    HardwareWalletUpdate,
    WalletRegistered(Result<(Fingerprint, Option<[u8; 32]>), Error>),
    MnemonicWord(usize, String),
    ImportMnemonic(bool),
    RedeemNextKey,
    KeyRedeemed(ProviderKey, Result<(), liana_connect::keys::Error>),
    AllKeysRedeemed,
    BackupDescriptor,
    BackupPrivacyMode(bool),
    ShowBackupDescriptorHelp(bool),
    ExportEncryptedDescriptor(Result<Box<LianaDescriptor>, bip138::Error>),
    ExportXpub(String),
    ImportExport(ImportExportMessage),
    ImportBackup,
    /// Talia Desktop: a vault read from a QR code with the camera.
    TaliaScan(crate::installer::step::talia_scan::ScanMessage),
    WalletFromBackup((HashMap<Fingerprint, settings::KeySetting>, Backup)),
    WalletAliasEdited(String),
    LanguageEdited(SupportedLocale),
    SelectAccount(Fingerprint, ChildNumber),
    OpenUrl(String),
    SelectKeySource(SelectKeySourceMessage),
    EditKeyAlias(EditKeyAliasMessage),
    Decrypt(Decrypt),
    None,
}

impl Close for Message {
    fn close() -> Self {
        Self::Close
    }
}

impl From<ImportExportMessage> for Message {
    fn from(value: ImportExportMessage) -> Self {
        Message::ImportExport(value)
    }
}

impl From<(Fingerprint, ChildNumber)> for Message {
    fn from(value: (Fingerprint, ChildNumber)) -> Self {
        Self::SelectAccount(value.0, value.1)
    }
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum SelectBackend {
    // view messages
    RequestOTP,
    EditEmail,
    ConnectWithAnotherEmail,
    BackToConnectAccounts,
    EmailEdited(String),
    OTPEdited(String),
    ContinueWithLocalWallet(bool),
    // Commands messages
    OTPRequested(Result<(AuthClient, String), Error>),
    OTPResent(Result<(), Error>),
    ExistingConnectAccounts(Vec<String>),
    SelectConnectAccount(String),
    Connected(Result<context::RemoteBackend, Error>),
}

#[derive(Debug, Clone)]
pub enum ImportRemoteWallet {
    ToggleOptions(bool),
    ToggleOption(ImportWalletOption),
    RemoteWallets(Result<Vec<liana_connect::wallets::api::Wallet>, Error>),
    ImportDescriptor(String),
    ConfirmDescriptor,
    ImportInvitationToken(String),
    FetchInvitation,
    InvitationFetched(Result<liana_connect::wallets::api::WalletInvitation, Error>),
    AcceptInvitation,
    InvitationAccepted(Result<liana_connect::wallets::api::Wallet, Error>),
    ImportDescriptorFromFile,
    ImportExport(ImportExportMessage),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportWalletOption {
    Invitation,
    PasteDescriptor,
}

#[derive(Debug, Clone)]
pub enum DefineBitcoind {
    ConfigFieldEdited(ConfigField, String),
    RpcAuthTypeSelected(RpcAuthType),
}

#[derive(Debug, Clone)]
pub enum DefineElectrum {
    ConfigFieldEdited(electrum::ConfigField, String),
    ValidDomainChanged(bool),
}

#[derive(Debug, Clone)]
pub enum DefineNode {
    NodeTypeSelected(NodeType),
    DefineBitcoind(DefineBitcoind),
    DefineElectrum(DefineElectrum),
    PingResult((NodeType, Result<(), Error>)),
    Ping,
}

#[derive(Debug, Clone)]
pub enum SelectBitcoindTypeMsg {
    UseExternal(bool),
}

#[derive(Debug, Clone)]
pub enum InternalBitcoindMsg {
    Previous,
    Reload,
    DefineConfig,
    Download,
    DownloadProgressed(Result<Progress, DownloadError>),
    Install,
    Start,
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum DefineDescriptor {
    ChangeTemplate(context::DescriptorTemplate),
    ImportDescriptor(String),
    // NOTE: KeysEdit & KeysEdited takes a Vec<coordinate>
    // in order to assign a key to several path from a single
    // modal call
    KeysEdited(Vec<(usize, usize)>, SelectedKey),
    KeysEdit(PathKind, Vec<(usize, usize)>),
    Path(usize, DefinePath),
    AddRecoveryPath,
    AddSafetyNetPath,
    ThresholdSequenceModal(ThresholdSequenceModal),
    Reset,
    ShowImportDescriptor(bool),
    AliasEdited(Fingerprint, String /* alias*/),
    Compiled(Result<Box<LianaDescriptor>, String>),
    Talia(TaliaMessage),
}

/// The choices of the iPhone app's vault shapes, and "Get my 21st Capital keys".
#[derive(Debug, Clone)]
pub enum TaliaMessage {
    /// Who holds the co-signer slot: 21st Capital (true) or the client.
    CosignerOurs(bool),
    /// The answer to the Rescue Key question.
    Rescue(bool),
    AddHeir,
    RemoveHeir(usize),
    HeirMonths(usize, u8),
    /// The 2 of 2's answer to "What happens if you lose one of your two keys?".
    Backup(bool),
    BackupKind(crate::installer::step::descriptor::editor::talia::BackupKind),
    /// Who holds the backup way in's added key: 21st Capital (true) or the client.
    BackupHolderOurs(bool),
    BackupMonths(u8),
    GetKeys,
    OpenKeysPage,
    CancelKeys,
    KeysFetched(u64, Result<Box<FetchedKeys>, HandoffError>),
    /// Takes the 21st Capital keys out, so the shape can change again.
    ChangeShape,
    /// Build your own: the client picked one of our keys for the slot at
    /// (path, key), the co-signer on the primary path or the Rescue Key.
    OurKey(usize, usize),
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum DefinePath {
    AddKey,
    Key(usize, DefineKey),
    ThresholdEdited(usize),
    SequenceEdited(u16),
    EditSequence,
    EditThreshold,
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum DefineKey {
    Delete,
    Edit,
    EditAlias,
    Clipboard(String),
    OpenUrl(String),
}

#[derive(Debug, Clone)]
pub enum ThresholdSequenceModal {
    ThresholdEdited(usize),
    SequenceEdited(String),
    Confirm,
}

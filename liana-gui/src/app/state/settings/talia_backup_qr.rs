//! Talia Desktop: the wallet backup as a moving QR code that Talia on the
//! iPhone scans to add the vault there, with its name, key names and labels
//! (decided 2026-11-15).
//!
//! The code carries Liana's own wallet backup, the file "Back up wallet"
//! writes, in BBQr frames (`crate::bbqr`), which the iPhone app already reads.
//! Only what the phone needs travels: the vault, not this computer's node
//! settings, transaction history or drafts. Nothing in it can move coins.

use std::{sync::Arc, time::Duration};

use iced::{widget::qr_code, Subscription, Task};

use liana_ui::widget::{modal::Modal, Element};

use crate::{
    app::{
        message::Message,
        view::{
            self,
            settings::talia_backup_qr::{window, Shown, TaliaBackupQrMessage},
        },
        wallet::Wallet,
        Config,
    },
    backup::Backup,
    bbqr,
    daemon::Daemon,
    dir::LianaDirectory,
};

use liana::miniscript::bitcoin::Network;

/// The iPhone app shows its own moving codes at this pace.
const FRAME_TIME: Duration = Duration::from_millis(300);

/// Backup entries that describe this computer, not the vault: Liana's
/// `CONFIG_KEY` (the node connection) and `SETTINGS_KEY` (this app's wallet
/// settings) in `backup.rs`.
const COMPUTER_ONLY: [&str; 2] = ["config", "settings"];

pub enum BackupQr {
    Preparing,
    Ready {
        frames: Vec<qr_code::Data>,
        shown: usize,
    },
    Failed(String),
}

impl BackupQr {
    /// Opens the window and starts building the code.
    pub fn open(
        datadir: LianaDirectory,
        network: Network,
        config: Arc<Config>,
        wallet: Arc<Wallet>,
        daemon: Arc<dyn Daemon + Sync + Send>,
    ) -> (Self, Task<Message>) {
        let task = Task::perform(
            frames(datadir, network, config, wallet, daemon),
            |result| Message::View(TaliaBackupQrMessage::Built(result).into()),
        );
        (Self::Preparing, task)
    }

    /// Whether the window stays open.
    pub fn update(&mut self, message: TaliaBackupQrMessage) -> bool {
        match message {
            TaliaBackupQrMessage::Open => {}
            TaliaBackupQrMessage::Close => return false,
            TaliaBackupQrMessage::Built(Ok(frames)) => {
                *self = match frames
                    .iter()
                    .map(|frame| {
                        qr_code::Data::with_error_correction(
                            frame,
                            qr_code::ErrorCorrection::Low,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()
                {
                    Ok(frames) => Self::Ready { frames, shown: 0 },
                    Err(e) => Self::Failed(e.to_string()),
                };
            }
            TaliaBackupQrMessage::Built(Err(e)) => *self = Self::Failed(e),
            TaliaBackupQrMessage::Tick => {
                if let Self::Ready { frames, shown } = self {
                    *shown = (*shown + 1) % frames.len();
                }
            }
        }
        true
    }

    pub fn subscription(&self) -> Subscription<Message> {
        match self {
            Self::Ready { frames, .. } if frames.len() > 1 => iced::time::every(FRAME_TIME)
                .map(|_| Message::View(TaliaBackupQrMessage::Tick.into())),
            _ => Subscription::none(),
        }
    }

    pub fn view<'a>(
        &'a self,
        content: Element<'a, view::Message>,
    ) -> Element<'a, view::Message> {
        let shown = match self {
            Self::Preparing => Shown::Preparing,
            Self::Ready { frames, shown } => Shown::Frame(&frames[*shown]),
            Self::Failed(e) => Shown::Failed(e),
        };
        Modal::new(content, window(shown))
            .on_blur(Some(TaliaBackupQrMessage::Close.into()))
            .into()
    }
}

/// The wallet backup's frames, as "Back up wallet" would write it.
async fn frames(
    datadir: LianaDirectory,
    network: Network,
    config: Arc<Config>,
    wallet: Arc<Wallet>,
    daemon: Arc<dyn Daemon + Sync + Send>,
) -> Result<Vec<String>, String> {
    // Nobody follows this backup's progress: it takes a moment.
    let (progress, _ignored) = tokio::sync::mpsc::unbounded_channel();
    let backup = Backup::from_app(datadir, network, config, wallet, daemon, &progress)
        .await
        .map_err(|e| e.to_string())?;
    let payload = payload(backup).map_err(|e| e.to_string())?;
    bbqr::encode(&payload, bbqr::FileType::Json).map_err(|e| e.to_string())
}

/// The backup the code carries, compact: the vault, its name, its keys'
/// names, its labels and where its addresses stand. This computer's settings,
/// the transactions (the phone reads them from the chain), drafts and coins
/// stay here.
pub fn payload(mut backup: Backup) -> Result<Vec<u8>, serde_json::Error> {
    backup
        .proprietary
        .retain(|key, _| !COMPUTER_ONLY.contains(&key.as_str()));
    for account in &mut backup.accounts {
        account.transactions.clear();
        account.psbts.clear();
        account.coins.clear();
        account.chain_tip = None;
        account
            .proprietary
            .retain(|key, _| !COMPUTER_ONLY.contains(&key.as_str()));
    }
    serde_json::to_vec(&backup)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_code_carries_the_vault_and_not_this_computer() {
        let raw = r#"{
            "name": "Liana",
            "alias": "Family vault",
            "accounts": [{
                "descriptor": "wsh(multi(2,[aabbccdd/48'/1'/0'/2']tpubD6NzVbkrYhZ4XYBa9huubPUVRzmGGJoEjFF4i93okdJUBSiDenCbHMAZkjWYWiWoruNEgXouXEdKBL9gDWuxem4gwJMBEs3TVhhNw7AybcA/<0;1>/*,[11223344/48'/1'/0'/2']tpubD6NzVbkrYhZ4XYBa9huubPUVRzmGGJoEjFF4i93okdJUBSiDenCbHMAZkjWYWiWoruNEgXouXEdKBL9gDWuxem4gwJMBEs3TVhhNw7AybcA/<0;1>/*))",
                "receive_index": 4,
                "keys": {"aabbccdd": {"key": "aabbccdd", "alias": "Ledger"}},
                "labels": [{"type": "tx", "ref": "f91d0a8a78462bc59398f2c5d7a84fcff491c26ba54c4833478b202796c8aafd", "label": "Rent"}],
                "transactions": ["0200000001"],
                "psbts": ["cHNidP8B"],
                "chain_tip": {"block_height": 100},
                "proprietary": {"config": {"node": "secret"}, "keep": 1}
            }],
            "network": "testnet4",
            "proprietary": {"liana_version": "16.0.0", "config": {"node": "secret"}, "settings": {"x": 1}},
            "version": 0
        }"#;
        let backup: Backup = serde_json::from_str(raw).unwrap();
        let sent: serde_json::Value = serde_json::from_slice(&payload(backup).unwrap()).unwrap();
        assert_eq!(sent["alias"], "Family vault");
        assert_eq!(sent["network"], "testnet4");
        assert_eq!(sent["proprietary"], serde_json::json!({"liana_version": "16.0.0"}));
        let account = &sent["accounts"][0];
        assert!(account["descriptor"].as_str().unwrap().starts_with("wsh(multi(2,"));
        assert_eq!(account["receive_index"], 4);
        assert_eq!(account["keys"]["aabbccdd"]["alias"], "Ledger");
        assert_eq!(account["labels"][0]["label"], "Rent");
        for gone in ["transactions", "psbts", "coins", "chain_tip"] {
            assert!(account.get(gone).is_none(), "{} travelled", gone);
        }
        assert_eq!(account["proprietary"], serde_json::json!({"keep": 1}));
        // And Liana still reads what the phone gets.
        let back: Backup = serde_json::from_value(sent).unwrap();
        assert_eq!(back.accounts[0].keys.len(), 1);
    }
}

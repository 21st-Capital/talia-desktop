use std::convert::From;

use iced::Length;

use liana_ui::{component::notification, widget::*};

use crate::{
    app::error::Error,
    t,
    daemon::{client::error::RpcErrorCode, DaemonError},
};

/// Simple warning message displayed to non technical user.
pub struct WarningMessage(String);

impl From<&Error> for WarningMessage {
    fn from(error: &Error) -> WarningMessage {
        match error {
            Error::Config(e) => WarningMessage(e.to_owned()),
            Error::Wallet(_) => WarningMessage(t!("warning-wallet-error")),
            Error::Daemon(e) => match e {
                DaemonError::Rpc(code, _) => {
                    if *code == RpcErrorCode::JSONRPC2_INVALID_PARAMS as i32 {
                        WarningMessage(t!("warning-invalid-fields"))
                    } else {
                        WarningMessage(t!("warning-internal-error"))
                    }
                }
                DaemonError::Http(Some(code), error) => {
                    WarningMessage(t!("warning-http-error-code", code = code.to_string(), error = error.to_string()))
                }
                DaemonError::Http(None, error) => WarningMessage(t!("warning-http-error", error = error.to_string())),
                DaemonError::Unexpected(_) => WarningMessage(t!("warning-unknown-error")),
                DaemonError::Start(_) => WarningMessage(t!("warning-daemon-start")),
                DaemonError::ClientNotSupported => {
                    WarningMessage(t!("warning-daemon-unsupported"))
                }
                DaemonError::NoAnswer | DaemonError::RpcSocket(..) => {
                    WarningMessage(t!("warning-daemon-communication"))
                }
                DaemonError::DaemonStopped => WarningMessage(t!("warning-daemon-stopped")),
                DaemonError::CoinSelectionError => {
                    WarningMessage(t!("warning-coin-selection"))
                }
                DaemonError::NotImplemented => {
                    WarningMessage(t!("warning-not-implemented"))
                }
            },
            Error::Unexpected(_) => WarningMessage(t!("warning-unknown-error")),
            Error::HardwareWallet(_) => WarningMessage(t!("warning-hardware-wallet-error")),
            Error::Desc(e) => WarningMessage(t!("warning-descriptor-analysis", error = e.to_string())),
            Error::Spend(e) => WarningMessage(t!("warning-spend-creation", error = e.to_string())),
            Error::ImportExport(e) => WarningMessage(format!("{e}")),
            Error::RestoreBackup(e) => WarningMessage(t!("warning-restore-backup", error = e.to_string())),
            Error::FiatPrice(e) => WarningMessage(t!("warning-fiat-price", error = e.to_string())),
            Error::NoAvailableCurrencies | Error::SaveLanguage(_) => {
                WarningMessage(error.to_string())
            }
        }
    }
}

impl std::fmt::Display for WarningMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

pub fn warn<'a, T: 'a + Clone>(error: Option<&Error>) -> Container<'a, T> {
    if let Some(w) = error {
        let message: WarningMessage = w.into();
        notification::warning(message.to_string(), w.to_string()).width(Length::Fill)
    } else {
        Container::new(Column::new()).width(Length::Fill)
    }
}

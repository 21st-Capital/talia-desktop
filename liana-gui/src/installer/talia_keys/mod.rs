//! "Get my 21st Capital keys": the desktop key handoff (contract v1,
//! decided 2026-11-14).
//!
//! Talia Desktop opens the 21st Capital website, where the client signs in with
//! two-factor (and pays the onboarding fee if it is still open); the website
//! sends the browser back to a listener on 127.0.0.1 with a one-time code, and
//! the app exchanges that code for the vault's public keys. The flow is shaped
//! like OAuth for native apps: a loopback redirect (RFC 8252) and PKCE with
//! S256 (RFC 7636). Only public keys travel, and no descriptor ever leaves the
//! app.

pub mod listener;
pub mod proof;

use std::{future::Future, str::FromStr, time::Duration};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use liana::miniscript::{
    bitcoin::{
        hashes::{sha256, Hash},
        Network,
    },
    descriptor::DescriptorPublicKey,
};
use serde::Deserialize;
use serde_json::Value;

use crate::{
    installer::{
        descriptor::{Key, KeySource},
        step::descriptor::editor::key::check_key_network,
    },
    t,
};
use listener::{BrowserPages, ListenError};
use proof::{issued_keys_problem, IssuedKey, Pin, ProofNet};

/// The backend the code is exchanged with.
pub const DEFAULT_API: &str = "https://backend.21stcapital.com";
/// The website the client signs in on. The bare host, never "www.": the
/// website answers on both, a browser keeps each one's sign-in apart, and our
/// sign-in e-mails (and so the client's session) use the bare host.
pub const DEFAULT_SITE: &str = "https://21stcapital.com";
/// Overrides [`DEFAULT_API`], for testing against another backend.
pub const API_ENV: &str = "TALIA_KEYS_API";
/// Overrides [`DEFAULT_SITE`], for testing against another website.
pub const SITE_ENV: &str = "TALIA_KEYS_SITE";

const SITE_PATH: &str = "/desktop/keys";
const EXCHANGE_PATH: &str = "/api/vaults/desktop-handoff/exchange/";
const EXCHANGE_TIMEOUT: Duration = Duration::from_secs(30);

/// How long the app waits for the browser to come back: an hour, room for an
/// onboarding fee paid in bitcoin on the way (decided 2026-11-14).
pub const WAIT: Duration = Duration::from_secs(60 * 60);

/// The vault shapes, in the words the iPhone app and the backend use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VaultConfig {
    Multi2of3,
    Multi2of2,
    Single,
}

impl VaultConfig {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Multi2of3 => "multi-2-3",
            Self::Multi2of2 => "multi-2-2",
            Self::Single => "single-1-1",
        }
    }
}

/// What the website is asked for: the shape of the vault being built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultShape {
    pub config: VaultConfig,
    /// A 21st Capital co-signer on the everyday path.
    pub cosigner: bool,
    /// The 21st Capital Rescue Key (15 months).
    pub rescue: bool,
    /// Each heir's wait, in whole months (8 to 15).
    pub heirs: Vec<u8>,
}

/// The PKCE pair and the state, kept in memory only.
#[derive(Clone)]
pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
    pub state: String,
}

impl std::fmt::Debug for Pkce {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never in a log.
        f.write_str("Pkce(..)")
    }
}

/// `base64url(SHA-256(verifier))`, without padding.
pub fn challenge_for(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(sha256::Hash::hash(verifier.as_bytes()).to_byte_array())
}

impl Pkce {
    fn from_bytes(verifier: [u8; 32], state: [u8; 32]) -> Self {
        let verifier = URL_SAFE_NO_PAD.encode(verifier);
        Self {
            challenge: challenge_for(&verifier),
            verifier,
            state: URL_SAFE_NO_PAD.encode(state),
        }
    }

    /// A fresh verifier and state, 32 bytes each from the system's CSPRNG.
    pub fn generate() -> Result<Self, liana::random::RandomnessError> {
        Ok(Self::from_bytes(
            liana::random::random_bytes()?,
            liana::random::random_bytes()?,
        ))
    }
}

/// The contract's name for a wallet network, if 21st Capital issues keys on it.
pub fn contract_network(network: Network) -> Option<ProofNet> {
    match network {
        Network::Bitcoin => Some(ProofNet::Mainnet),
        Network::Testnet4 => Some(ProofNet::Testnet4),
        _ => None,
    }
}

/// The page that signs the client in and sends the browser back.
pub fn website_url(
    site: &str,
    network: ProofNet,
    shape: &VaultShape,
    pkce: &Pkce,
    port: u16,
) -> String {
    let mut heirs = shape.heirs.clone();
    heirs.sort_unstable();
    let heirs: Vec<String> = heirs.iter().map(u8::to_string).collect();
    format!(
        "{site}{SITE_PATH}?v=1&network={}&config={}&cosigner={}&rescue={}&heirs={}&challenge={}&state={}&port={port}",
        network.as_str(),
        shape.config.as_str(),
        u8::from(shape.cosigner),
        u8::from(shape.rescue),
        heirs.join(","),
        pkce.challenge,
        pkce.state,
    )
}

/// Why no keys came back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandoffError {
    /// An override names something other than an https address.
    BadSetting(&'static str),
    UnsupportedNetwork,
    Randomness,
    Listen(String),
    TimedOut,
    /// The code was refused: expired, already used, or not ours.
    InvalidCode,
    TooManyTries,
    Unreachable,
    BadAnswer(String),
    WrongNetwork {
        theirs: ProofNet,
        ours: ProofNet,
    },
    /// A key the vault needs did not come back.
    Missing,
    /// A key did not pass the key check (the reason is for the log only).
    CheckFailed(String),
}

fn network_name(network: ProofNet) -> String {
    match network {
        ProofNet::Mainnet => "Bitcoin".to_string(),
        ProofNet::Testnet4 => "Testnet4".to_string(),
    }
}

impl HandoffError {
    /// What the client reads.
    pub fn message(&self) -> String {
        match self {
            Self::BadSetting(name) => t!("installer-talia-keys-bad-setting", name = *name),
            Self::UnsupportedNetwork => t!("installer-talia-keys-network-unsupported"),
            Self::Randomness | Self::Listen(_) => t!("installer-talia-keys-listen-failed"),
            Self::TimedOut => t!("installer-talia-keys-timeout"),
            Self::InvalidCode => t!("installer-talia-keys-invalid-code"),
            Self::TooManyTries => t!("installer-talia-keys-too-many-tries"),
            Self::Unreachable => t!("installer-talia-keys-unreachable"),
            Self::BadAnswer(_) => t!("installer-talia-keys-bad-answer"),
            Self::WrongNetwork { theirs, ours } => t!(
                "installer-talia-keys-wrong-network",
                theirs = network_name(*theirs),
                ours = network_name(*ours)
            ),
            Self::Missing => t!("installer-talia-keys-missing"),
            Self::CheckFailed(_) => t!("installer-talia-keys-check-failed"),
        }
    }
}

impl From<ListenError> for HandoffError {
    fn from(e: ListenError) -> Self {
        match e {
            ListenError::TimedOut => Self::TimedOut,
            ListenError::Io(e) => Self::Listen(e),
        }
    }
}

/// An endpoint from its override, else the default: https, or plain http on
/// the loopback only (a local test server).
fn endpoint(
    name: &'static str,
    value: Option<String>,
    default: &str,
) -> Result<String, HandoffError> {
    let Some(value) = value.filter(|v| !v.trim().is_empty()) else {
        return Ok(default.to_string());
    };
    let url = reqwest::Url::parse(value.trim()).map_err(|_| HandoffError::BadSetting(name))?;
    let loopback = matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"));
    let allowed = url.scheme() == "https" || (url.scheme() == "http" && loopback);
    if !allowed || url.query().is_some() || url.fragment().is_some() {
        return Err(HandoffError::BadSetting(name));
    }
    Ok(url.as_str().trim_end_matches('/').to_string())
}

/// The 21st Capital keys of a new vault, ready for its slots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchedKeys {
    pub network: ProofNet,
    pub vault_number: Option<u64>,
    pub account: Option<u64>,
    pub cosigner: Option<Key>,
    pub rescue: Option<Key>,
}

#[derive(Deserialize)]
struct RawKey {
    fingerprint: Option<String>,
    origin_path: Option<String>,
    xpub: Option<String>,
    #[serde(default)]
    proof: Option<Value>,
}

impl RawKey {
    fn issued(self) -> Option<IssuedKey> {
        let fingerprint = self.fingerprint.filter(|f| !f.is_empty())?;
        let xpub = self.xpub.filter(|x| !x.is_empty())?;
        Some(IssuedKey {
            fingerprint: fingerprint.to_lowercase(),
            origin_path: self.origin_path.unwrap_or_default(),
            xpub,
            proof: self.proof,
        })
    }
}

#[derive(Deserialize)]
struct ExchangeAnswer {
    network: String,
    #[serde(default)]
    vault_number: Option<u64>,
    #[serde(default)]
    account: Option<u64>,
    #[serde(default)]
    co_signer: Option<RawKey>,
    #[serde(default)]
    rescue: Option<RawKey>,
}

/// `[fingerprint/origin]xpub`, the way Liana takes a key it cannot fetch
/// from a device.
fn descriptor_key(key: &IssuedKey, network: Network) -> Result<DescriptorPublicKey, HandoffError> {
    let origin = key.origin_path.trim().replace(['h', 'H'], "'");
    let origin = origin.strip_prefix("m/").unwrap_or(&origin);
    let text = format!("[{}/{}]{}", key.fingerprint, origin, key.xpub.trim());
    let parsed = DescriptorPublicKey::from_str(&text)
        .map_err(|e| HandoffError::BadAnswer(format!("unreadable key: {e}")))?;
    match &parsed {
        DescriptorPublicKey::XPub(xpub)
            if xpub.derivation_path.is_master() && check_key_network(&parsed, network) =>
        {
            Ok(parsed)
        }
        _ => Err(HandoffError::BadAnswer("key of the wrong kind".to_string())),
    }
}

fn slot_key(key: &IssuedKey, network: Network, name: String) -> Result<Key, HandoffError> {
    let key_ = descriptor_key(key, network)?;
    Ok(Key {
        source: KeySource::Manual,
        name,
        fingerprint: key_.master_fingerprint(),
        key: key_,
        account: None,
    })
}

/// Reads the exchange answer and checks every key the vault needs: the
/// network, the presence of each, and the key check (fingerprint pins and
/// the device-signed proof). Keys the vault does not need are ignored.
pub fn parse_answer(
    body: &str,
    network: Network,
    shape: &VaultShape,
    pins: &[Pin],
) -> Result<FetchedKeys, HandoffError> {
    let ours = contract_network(network).ok_or(HandoffError::UnsupportedNetwork)?;
    let answer: ExchangeAnswer =
        serde_json::from_str(body).map_err(|e| HandoffError::BadAnswer(e.to_string()))?;
    let theirs =
        ProofNet::from_contract(&answer.network).ok_or(HandoffError::UnsupportedNetwork)?;
    if theirs != ours {
        return Err(HandoffError::WrongNetwork { theirs, ours });
    }
    let cosigner = if shape.cosigner {
        Some(
            answer
                .co_signer
                .and_then(RawKey::issued)
                .ok_or(HandoffError::Missing)?,
        )
    } else {
        None
    };
    let rescue = if shape.rescue {
        Some(
            answer
                .rescue
                .and_then(RawKey::issued)
                .ok_or(HandoffError::Missing)?,
        )
    } else {
        None
    };
    if let Some(problem) = issued_keys_problem(
        cosigner.as_ref(),
        rescue.as_ref(),
        ours,
        answer.account,
        pins,
    ) {
        tracing::error!("The 21st Capital key check did not pass: {problem}");
        return Err(HandoffError::CheckFailed(problem));
    }
    Ok(FetchedKeys {
        network: ours,
        vault_number: answer.vault_number,
        account: answer.account,
        cosigner: cosigner
            .map(|k| slot_key(&k, network, t!("installer-talia-cosigner")))
            .transpose()?,
        rescue: rescue
            .map(|k| slot_key(&k, network, t!("installer-talia-rescue-key")))
            .transpose()?,
    })
}

/// Trades the one-time code and the verifier for the answer's body.
pub async fn exchange(api: &str, code: &str, verifier: &str) -> Result<String, HandoffError> {
    let client = reqwest::Client::builder()
        .timeout(EXCHANGE_TIMEOUT)
        // The code and the verifier go to this one address, never elsewhere.
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| HandoffError::Unreachable)?;
    let response = client
        .post(format!("{api}{EXCHANGE_PATH}"))
        .json(&serde_json::json!({ "code": code, "verifier": verifier }))
        .send()
        .await
        .map_err(|e| {
            tracing::warn!("Key exchange failed: {e}");
            HandoffError::Unreachable
        })?;
    let status = response.status();
    if status == reqwest::StatusCode::BAD_REQUEST {
        return Err(HandoffError::InvalidCode);
    }
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Err(HandoffError::TooManyTries);
    }
    if !status.is_success() {
        tracing::warn!("Key exchange answered {status}");
        return Err(HandoffError::Unreachable);
    }
    response.text().await.map_err(|_| HandoffError::Unreachable)
}

/// One handoff, started: the page to open, and the wait for the keys. Dropping
/// the wait stops the listening.
pub struct Started<F> {
    pub url: String,
    pub keys: F,
}

/// Where the handoff goes and how long it waits.
pub struct Endpoints {
    pub site: String,
    pub api: String,
    pub wait: Duration,
}

impl Endpoints {
    /// The real website and backend, or their overrides.
    pub fn from_env() -> Result<Self, HandoffError> {
        Ok(Self {
            site: endpoint(SITE_ENV, std::env::var(SITE_ENV).ok(), DEFAULT_SITE)?,
            api: endpoint(API_ENV, std::env::var(API_ENV).ok(), DEFAULT_API)?,
            wait: WAIT,
        })
    }
}

/// Binds the loopback listener and builds the page to open. The returned wait
/// serves the listener until the browser comes back, exchanges the code and
/// checks the keys.
pub fn start(
    network: Network,
    shape: VaultShape,
    pages: BrowserPages,
    endpoints: Endpoints,
    pins: Vec<Pin>,
) -> Result<Started<impl Future<Output = Result<FetchedKeys, HandoffError>>>, HandoffError> {
    let net = contract_network(network).ok_or(HandoffError::UnsupportedNetwork)?;
    let pkce = Pkce::generate().map_err(|_| HandoffError::Randomness)?;
    let std_listener =
        listener::bind_loopback().map_err(|e| HandoffError::Listen(e.to_string()))?;
    let port = std_listener
        .local_addr()
        .map_err(|e| HandoffError::Listen(e.to_string()))?
        .port();
    let url = website_url(&endpoints.site, net, &shape, &pkce, port);
    let keys = async move {
        let deadline = tokio::time::Instant::now() + endpoints.wait;
        let listener = tokio::net::TcpListener::from_std(std_listener)
            .map_err(|e| HandoffError::Listen(e.to_string()))?;
        let code = listener::wait_for_code(listener, &pkce.state, deadline, &pages).await?;
        let body = exchange(&endpoints.api, &code, &pkce.verifier).await?;
        parse_answer(&body, network, &shape, &pins)
    };
    Ok(Started { url, keys })
}

#[cfg(test)]
mod tests {
    use super::*;
    use liana::miniscript::bitcoin::{
        base58,
        bip32::{DerivationPath, Xpriv, Xpub},
        hashes::hash160,
        secp256k1::{Message, Secp256k1, SecretKey},
        sign_message::signed_msg_hash,
        NetworkKind,
    };
    use proof::{leaf_hash, Purpose};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn pkce_matches_rfc_7636() {
        // RFC 7636, appendix B.
        assert_eq!(
            challenge_for("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        let pkce = Pkce::generate().unwrap();
        assert_eq!(pkce.verifier.len(), 43);
        assert_eq!(pkce.state.len(), 43);
        assert_eq!(pkce.challenge.len(), 43);
        assert_eq!(pkce.challenge, challenge_for(&pkce.verifier));
        assert_eq!(URL_SAFE_NO_PAD.decode(&pkce.verifier).unwrap().len(), 32);
        let other = Pkce::generate().unwrap();
        assert_ne!(pkce.verifier, other.verifier);
        assert_ne!(pkce.state, other.state);
        assert_ne!(pkce.verifier, pkce.state);
        assert_eq!(format!("{pkce:?}"), "Pkce(..)");
    }

    #[test]
    fn the_website_url_follows_the_contract() {
        let pkce = Pkce::from_bytes([1; 32], [2; 32]);
        let shape = VaultShape {
            config: VaultConfig::Multi2of3,
            cosigner: true,
            rescue: true,
            heirs: vec![14, 9],
        };
        assert_eq!(
            website_url(DEFAULT_SITE, ProofNet::Mainnet, &shape, &pkce, 50123),
            format!(
                "https://21stcapital.com/desktop/keys?v=1&network=mainnet&config=multi-2-3\
                 &cosigner=1&rescue=1&heirs=9,14&challenge={}&state={}&port=50123",
                pkce.challenge, pkce.state
            )
        );
        let single = VaultShape {
            config: VaultConfig::Single,
            cosigner: false,
            rescue: true,
            heirs: vec![],
        };
        let url = website_url(
            "http://127.0.0.1:8000",
            ProofNet::Testnet4,
            &single,
            &pkce,
            1024,
        );
        assert!(url.starts_with("http://127.0.0.1:8000/desktop/keys?v=1&network=testnet4"));
        assert!(url.contains("&config=single-1-1&cosigner=0&rescue=1&heirs=&challenge="));
        assert!(url.ends_with("&port=1024"));
    }

    #[test]
    fn only_bitcoin_and_testnet4_have_our_keys() {
        assert_eq!(contract_network(Network::Bitcoin), Some(ProofNet::Mainnet));
        assert_eq!(
            contract_network(Network::Testnet4),
            Some(ProofNet::Testnet4)
        );
        assert_eq!(contract_network(Network::Signet), None);
        assert_eq!(contract_network(Network::Testnet), None);
        assert_eq!(contract_network(Network::Regtest), None);
    }

    #[test]
    fn overrides_must_be_https_or_the_loopback() {
        assert_eq!(endpoint(API_ENV, None, DEFAULT_API).unwrap(), DEFAULT_API);
        assert_eq!(
            endpoint(API_ENV, Some(" ".into()), DEFAULT_API).unwrap(),
            DEFAULT_API
        );
        assert_eq!(
            endpoint(
                API_ENV,
                Some("https://staging.example.com/".into()),
                DEFAULT_API
            )
            .unwrap(),
            "https://staging.example.com"
        );
        assert_eq!(
            endpoint(API_ENV, Some("http://127.0.0.1:8000".into()), DEFAULT_API).unwrap(),
            "http://127.0.0.1:8000"
        );
        for bad in [
            "http://example.com",
            "ftp://127.0.0.1",
            "not a url",
            "https://x.com/?a=b",
        ] {
            assert_eq!(
                endpoint(API_ENV, Some(bad.into()), DEFAULT_API),
                Err(HandoffError::BadSetting(API_ENV)),
                "{bad}"
            );
        }
    }

    /// A test pool: keys of throwaway seeds, a tree over them, a manifest
    /// signed by a throwaway attestation key, and the pin of that key.
    struct Pool {
        purpose: Purpose,
        seed_fingerprint: String,
        first: u64,
        xpubs: Vec<String>,
        attest: SecretKey,
    }

    impl Pool {
        fn new(purpose: Purpose, seed: u8, attest: u8, first: u64, count: u64) -> Self {
            let secp = Secp256k1::new();
            let master = Xpriv::new_master(NetworkKind::Test, &[seed; 32]).unwrap();
            let xpubs = (first..first + count)
                .map(|account| {
                    let path =
                        DerivationPath::from_str(&format!("m/48'/1'/{account}'/2'")).unwrap();
                    Xpub::from_priv(&secp, &master.derive_priv(&secp, &path).unwrap()).to_string()
                })
                .collect();
            Self {
                purpose,
                seed_fingerprint: master.fingerprint(&secp).to_string(),
                first,
                xpubs,
                attest: SecretKey::from_slice(&[attest; 32]).unwrap(),
            }
        }

        fn pin(&self) -> Pin {
            let secp = Secp256k1::new();
            let pubkey = self.attest.public_key(&secp).serialize();
            let mut data = vec![0x6f];
            data.extend_from_slice(hash160::Hash::hash(&pubkey).as_byte_array());
            Pin {
                network: ProofNet::Testnet4,
                purpose: self.purpose,
                seed: self.seed_fingerprint.clone(),
                address: base58::encode_check(&data),
            }
        }

        /// The key of `account`, with its proof, as the backend serves it.
        fn served(&self, account: u64) -> Value {
            let leaves: Vec<[u8; 32]> = self
                .xpubs
                .iter()
                .enumerate()
                .map(|(i, x)| {
                    leaf_hash(
                        self.first + i as u64,
                        &self.seed_fingerprint,
                        x,
                        self.purpose,
                        1,
                    )
                })
                .collect();
            let index = (account - self.first) as usize;
            let (root, path) = tree(&leaves, index);
            let last = self.first + self.xpubs.len() as u64 - 1;
            let manifest = format!(
                "21C key pool v2\npurpose {}\nseed {}\nchain 48'/1'\naccounts {}-{}\nkeys {}\nroot {}",
                self.purpose.as_str(),
                self.seed_fingerprint,
                self.first,
                last,
                self.xpubs.len(),
                hex::encode(root)
            );
            let secp = Secp256k1::new();
            let digest = signed_msg_hash(&manifest).to_byte_array();
            let sig = secp.sign_ecdsa_recoverable(&Message::from_digest(digest), &self.attest);
            let (recid, compact) = sig.serialize_compact();
            let mut raw = vec![31 + recid.to_i32() as u8];
            raw.extend_from_slice(&compact);
            serde_json::json!({
                "fingerprint": self.seed_fingerprint,
                "origin_path": format!("48'/1'/{account}'/2'"),
                "xpub": self.xpubs[index],
                "proof": {
                    "format": proof::PROOF_FORMAT,
                    "manifest": manifest,
                    "signature": base64::engine::general_purpose::STANDARD.encode(raw),
                    "index": index,
                    "path": path.iter().map(hex::encode).collect::<Vec<_>>(),
                },
            })
        }
    }

    /// The root and the audit path of leaf `index` (RFC 9162).
    fn tree(leaves: &[[u8; 32]], index: usize) -> ([u8; 32], Vec<[u8; 32]>) {
        let node = |l: &[u8; 32], r: &[u8; 32]| {
            let mut data = vec![1u8];
            data.extend_from_slice(l);
            data.extend_from_slice(r);
            sha256::Hash::hash(&data).to_byte_array()
        };
        let mut path = Vec::new();
        let mut level: Vec<([u8; 32], Vec<usize>)> = leaves
            .iter()
            .enumerate()
            .map(|(i, h)| (*h, vec![i]))
            .collect();
        while level.len() > 1 {
            let mut next = Vec::new();
            for pair in level.chunks(2) {
                match pair {
                    [(l, lm), (r, rm)] => {
                        if lm.contains(&index) {
                            path.push(*r);
                        }
                        if rm.contains(&index) {
                            path.push(*l);
                        }
                        next.push((node(l, r), [lm.clone(), rm.clone()].concat()));
                    }
                    [lone] => next.push(lone.clone()),
                    _ => unreachable!(),
                }
            }
            level = next;
        }
        (level[0].0, path)
    }

    fn pools() -> (Pool, Pool, Vec<Pin>) {
        let cosigner = Pool::new(Purpose::Cosigner, 11, 21, 1000, 5);
        let rescue = Pool::new(Purpose::Rescue, 12, 22, 1000, 5);
        let pins = vec![cosigner.pin(), rescue.pin()];
        (cosigner, rescue, pins)
    }

    fn two_of_three() -> VaultShape {
        VaultShape {
            config: VaultConfig::Multi2of3,
            cosigner: true,
            rescue: true,
            heirs: vec![12],
        }
    }

    fn answer(cosigner: &Pool, rescue: &Pool, account: u64) -> Value {
        serde_json::json!({
            "network": "testnet4",
            "vault_number": 2,
            "account": account,
            "co_signer": cosigner.served(account),
            "rescue": rescue.served(account),
        })
    }

    #[test]
    fn a_good_answer_fills_both_slots() {
        let (cosigner, rescue, pins) = pools();
        let body = answer(&cosigner, &rescue, 1003).to_string();
        let keys = parse_answer(&body, Network::Testnet4, &two_of_three(), &pins).unwrap();
        assert_eq!(keys.network, ProofNet::Testnet4);
        assert_eq!(keys.vault_number, Some(2));
        assert_eq!(keys.account, Some(1003));
        let co = keys.cosigner.unwrap();
        assert_eq!(co.name, "21st Capital co-signer");
        assert_eq!(co.fingerprint.to_string(), cosigner.seed_fingerprint);
        assert_eq!(
            co.key.to_string(),
            format!(
                "[{}/48'/1'/1003'/2']{}",
                cosigner.seed_fingerprint, cosigner.xpubs[3]
            )
        );
        assert!(co.source.is_manual());
        let re = keys.rescue.unwrap();
        assert_eq!(re.name, "21st Capital Rescue Key");
        assert_eq!(re.fingerprint.to_string(), rescue.seed_fingerprint);
    }

    #[test]
    fn keys_the_vault_does_not_need_are_left_out() {
        let (cosigner, rescue, pins) = pools();
        let body = answer(&cosigner, &rescue, 1000).to_string();
        let single = VaultShape {
            config: VaultConfig::Single,
            cosigner: false,
            rescue: true,
            heirs: vec![],
        };
        let keys = parse_answer(&body, Network::Testnet4, &single, &pins).unwrap();
        assert!(keys.cosigner.is_none());
        assert!(keys.rescue.is_some());
    }

    #[test]
    fn bad_answers_are_refused() {
        let (cosigner, rescue, pins) = pools();
        let shape = two_of_three();

        // Another network than the wallet's.
        let body = answer(&cosigner, &rescue, 1001).to_string();
        assert_eq!(
            parse_answer(&body, Network::Bitcoin, &shape, &pins),
            Err(HandoffError::WrongNetwork {
                theirs: ProofNet::Testnet4,
                ours: ProofNet::Mainnet
            })
        );
        // A wallet network we have no keys on.
        assert_eq!(
            parse_answer(&body, Network::Signet, &shape, &pins),
            Err(HandoffError::UnsupportedNetwork)
        );
        // A network the contract does not know.
        let mut odd = answer(&cosigner, &rescue, 1001);
        odd["network"] = "signet".into();
        assert_eq!(
            parse_answer(&odd.to_string(), Network::Testnet4, &shape, &pins),
            Err(HandoffError::UnsupportedNetwork)
        );
        // A key the vault needs is missing.
        let mut missing = answer(&cosigner, &rescue, 1001);
        missing["rescue"] = Value::Null;
        assert_eq!(
            parse_answer(&missing.to_string(), Network::Testnet4, &shape, &pins),
            Err(HandoffError::Missing)
        );
        // Not JSON, or not the answer.
        assert!(matches!(
            parse_answer("<html>", Network::Testnet4, &shape, &pins),
            Err(HandoffError::BadAnswer(_))
        ));
        // The keys swapped between the roles: the proofs are for the other purpose.
        let mut swapped = answer(&cosigner, &rescue, 1001);
        let co = swapped["co_signer"].take();
        swapped["co_signer"] = swapped["rescue"].take();
        swapped["rescue"] = co;
        assert!(matches!(
            parse_answer(&swapped.to_string(), Network::Testnet4, &shape, &pins),
            Err(HandoffError::CheckFailed(_))
        ));
        // A key whose xpub was replaced on the way.
        let mut replaced = answer(&cosigner, &rescue, 1001);
        replaced["rescue"]["xpub"] = rescue.xpubs[4].clone().into();
        match parse_answer(&replaced.to_string(), Network::Testnet4, &shape, &pins) {
            Err(HandoffError::CheckFailed(reason)) => {
                assert_eq!(reason, "rescue key: proof invalid (PROOF_PATH)")
            }
            other => panic!("{:?}", other),
        }
        // The account the server names is not the key's.
        let mut account = answer(&cosigner, &rescue, 1001);
        account["account"] = 1002.into();
        assert!(matches!(
            parse_answer(&account.to_string(), Network::Testnet4, &shape, &pins),
            Err(HandoffError::CheckFailed(_))
        ));
        // Without the pins, a well-signed pool is still not ours.
        assert!(matches!(
            parse_answer(
                &body_for(&cosigner, &rescue),
                Network::Testnet4,
                &shape,
                &[]
            ),
            Err(HandoffError::CheckFailed(_))
        ));
        // Without its proof, a key is refused.
        let mut unproven = answer(&cosigner, &rescue, 1001);
        unproven["co_signer"]["proof"] = Value::Null;
        match parse_answer(&unproven.to_string(), Network::Testnet4, &shape, &pins) {
            Err(HandoffError::CheckFailed(reason)) => {
                assert_eq!(reason, "co-signing key: proof missing")
            }
            other => panic!("{:?}", other),
        }
    }

    fn body_for(cosigner: &Pool, rescue: &Pool) -> String {
        answer(cosigner, rescue, 1001).to_string()
    }

    /// A one-request HTTP server standing in for the backend. It answers
    /// `answer` and hands back the request the app made.
    async fn fake_backend(answer: String) -> (u16, tokio::task::JoinHandle<Value>) {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = Vec::new();
            let mut chunk = [0u8; 4096];
            let body = loop {
                let n = stream.read(&mut chunk).await.unwrap();
                assert!(n > 0, "the request ended early");
                buf.extend_from_slice(&chunk[..n]);
                let text = String::from_utf8_lossy(&buf).to_string();
                if let Some((head, body)) = text.split_once("\r\n\r\n") {
                    assert!(
                        head.starts_with("POST /api/vaults/desktop-handoff/exchange/ HTTP/1.1"),
                        "{}",
                        head
                    );
                    let length: usize = head
                        .lines()
                        .find_map(|l| {
                            let (name, value) = l.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse().unwrap())
                        })
                        .unwrap();
                    if body.len() >= length {
                        break body.to_string();
                    }
                }
            };
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}",
                answer.len()
            );
            stream.write_all(response.as_bytes()).await.unwrap();
            serde_json::from_str(&body).unwrap()
        });
        (port, handle)
    }

    fn query(url: &str, name: &str) -> String {
        reqwest::Url::parse(url)
            .unwrap()
            .query_pairs()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.into_owned())
            .unwrap()
    }

    #[tokio::test]
    async fn the_whole_handoff_against_local_fakes() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let (cosigner, rescue, pins) = pools();
        let (api_port, backend) = fake_backend(answer(&cosigner, &rescue, 1002).to_string()).await;

        let started = start(
            Network::Testnet4,
            two_of_three(),
            BrowserPages {
                title: "Talia Desktop".into(),
                done: "Your keys are on their way to Talia Desktop. You can close this tab.".into(),
                wrong_request: "Start again from Talia Desktop.".into(),
            },
            Endpoints {
                site: DEFAULT_SITE.into(),
                api: format!("http://127.0.0.1:{api_port}"),
                wait: Duration::from_secs(20),
            },
            pins,
        )
        .unwrap();

        // The website's side: it reads the challenge, the state and the port.
        let url = started.url.clone();
        assert!(url.starts_with("https://21stcapital.com/desktop/keys?v=1&network=testnet4"));
        assert_eq!(query(&url, "config"), "multi-2-3");
        assert_eq!(query(&url, "heirs"), "12");
        let challenge = query(&url, "challenge");
        let state = query(&url, "state");
        let port: u16 = query(&url, "port").parse().unwrap();
        assert!(port >= 1024);

        let app = tokio::spawn(started.keys);

        // The browser comes back with the code.
        let code = "T25lVGltZUNvZGVGb3JUaGVIYW5kb2ZmVGVzdDEyMzQ1";
        let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .unwrap();
        stream
            .write_all(
                format!(
                    "GET /talia/keys?code={code}&state={state} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n"
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        let mut page = String::new();
        stream.read_to_string(&mut page).await.unwrap();
        assert!(page.starts_with("HTTP/1.1 200"), "{}", page);
        assert!(page.contains("You can close this tab."));

        let keys = app.await.unwrap().unwrap();
        assert_eq!(keys.account, Some(1002));
        assert!(keys.cosigner.is_some() && keys.rescue.is_some());

        // The app sent the code and the verifier behind the challenge.
        let request = backend.await.unwrap();
        assert_eq!(request["code"], code);
        assert_eq!(
            challenge_for(request["verifier"].as_str().unwrap()),
            challenge
        );
    }

    #[tokio::test]
    async fn a_refused_code_reads_as_invalid() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut chunk = [0u8; 4096];
            let _ = stream.read(&mut chunk).await;
            let body = r#"{"error": "INVALID_CODE"}"#;
            let response = format!(
                "HTTP/1.1 400 Bad Request\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes()).await;
        });
        assert_eq!(
            exchange(&format!("http://127.0.0.1:{port}"), "code", "verifier").await,
            Err(HandoffError::InvalidCode)
        );
    }

    #[test]
    fn the_wait_leaves_room_for_a_bitcoin_payment() {
        assert_eq!(WAIT, Duration::from_secs(3600));
        assert_eq!(Endpoints::from_env().map(|e| e.wait), Ok(WAIT));
    }
}

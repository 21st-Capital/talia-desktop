//! What Talia Desktop checks on the 21st Capital keys it is handed, before
//! either goes into a new vault. A port of the iPhone app's checks (the
//! fingerprint pins and the device-signed per-key proof, decided 2026-09-26),
//! byte for byte the same contract, held to the same golden vectors.
//!
//! The rescue key alone spends every coin once its lock opens, so a key
//! swapped on the way (a compromised server, a pool imported under the wrong
//! purpose, an intercepted connection) would sweep the vault later with
//! nothing looking wrong. Here:
//!  - each key is a well-formed extended public key of the vault's network, at
//!    depth four, on the multisig purpose 48'/coin'/account'/2';
//!  - on Bitcoin, the co-signer comes from 21st Capital's live co-signing seed
//!    and the rescue key from its live rescue seed, pinned by fingerprint;
//!  - each key carries a proof: the device holding the seed signed (BIP-137,
//!    P2PKH at m/44'/coin'/99'/0/0) a manifest whose last line is the root of
//!    a Merkle tree (RFC 9162) over every key of the pool. The key's leaf is
//!    rebuilt from the key itself, walked up its audit path to the root, and
//!    the signer's address is compared with the address pinned below, which
//!    was read off the device's own screen.
//!
//! The iPhone app enforces the proof on both networks and requires it for
//! both roles since 2026-10-14; so does Talia Desktop, from its first build:
//! anything but a verified proof refuses the key.
//!
//! WHEN 21st Capital MOVES TO A NEW SEED, its fingerprint and attestation
//! address are added here (and in the iPhone app) in the same release that
//! starts issuing from it, and the old ones removed.

use std::fmt;

use base64::{engine::general_purpose::STANDARD, Engine};
use liana::miniscript::bitcoin::{
    base58,
    hashes::{hash160, sha256, Hash},
    secp256k1::{
        ecdsa::{RecoverableSignature, RecoveryId},
        Message, Secp256k1,
    },
    sign_message::signed_msg_hash,
};
use serde_json::Value;

/// The network a 21st Capital key is issued for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProofNet {
    Mainnet,
    Testnet4,
}

impl ProofNet {
    /// The BIP-44 coin type of the network.
    pub fn coin(self) -> u64 {
        match self {
            Self::Mainnet => 0,
            Self::Testnet4 => 1,
        }
    }

    /// The contract's name for the network.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mainnet => "mainnet",
            Self::Testnet4 => "testnet4",
        }
    }

    pub fn from_contract(s: &str) -> Option<Self> {
        match s {
            "mainnet" => Some(Self::Mainnet),
            "testnet4" => Some(Self::Testnet4),
            _ => None,
        }
    }
}

/// What a 21st Capital key is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    Cosigner,
    Rescue,
}

impl Purpose {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cosigner => "cosigner",
            Self::Rescue => "rescue",
        }
    }

    fn from_manifest(s: &str) -> Option<Self> {
        match s {
            "cosigner" => Some(Self::Cosigner),
            "rescue" => Some(Self::Rescue),
            _ => None,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Cosigner => "co-signing key",
            Self::Rescue => "rescue key",
        }
    }
}

/// 21st Capital's live Bitcoin seeds, by role (fingerprints, lowercase). Test
/// networks are rotated often and are not pinned by fingerprint; the proof
/// pins below cover them.
const PINNED_MAINNET_COSIGNER: &[&str] = &["bb09d891"];
const PINNED_MAINNET_RESCUE: &[&str] = &["fdcd357b"];

/// One attestation pin: the P2PKH address the seed's device showed for its
/// attestation key, per (network, purpose, seed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pin {
    pub network: ProofNet,
    pub purpose: Purpose,
    pub seed: String,
    pub address: String,
}

/// The attestation addresses pinned in Talia Desktop: the same four as the
/// iPhone app's and the backend's.
pub fn default_pins() -> Vec<Pin> {
    let pin = |network, purpose, seed: &str, address: &str| Pin {
        network,
        purpose,
        seed: seed.to_string(),
        address: address.to_string(),
    };
    vec![
        // Live co-signing seed.
        pin(
            ProofNet::Mainnet,
            Purpose::Cosigner,
            "bb09d891",
            "15Gtwbpj23Ey4sMLscUbDAMAzV7YoGnoeF",
        ),
        // Live rescue seed.
        pin(
            ProofNet::Mainnet,
            Purpose::Rescue,
            "fdcd357b",
            "1EK1qTm9nBTJjJA1saZ4xCpXZ9X2A1YPiq",
        ),
        pin(
            ProofNet::Testnet4,
            Purpose::Cosigner,
            "49578625",
            "mfvPbW2wVp3SZ9cezoRFKb6TFoY3qHXU7P",
        ),
        // A passphrase wallet: its address was read with the passphrase entered.
        pin(
            ProofNet::Testnet4,
            Purpose::Rescue,
            "01d4b72e",
            "mqFFvB2KKzWHTUkfFPM9CgEwp2nT5LqaGU",
        ),
    ]
}

pub const PROOF_FORMAT: &str = "21c-key-proof-v1";
/// A tree of 2^32 keys needs 32 hashes; nothing longer is read.
const MAX_PROOF_PATH: usize = 32;
/// Accounts are hardened indexes: nothing above 2^31 - 1.
const MAX_ACCOUNT: u64 = 0x7fff_ffff;
/// A proof object has exactly these fields, in this (sorted) order.
const PROOF_FIELDS: [&str; 5] = ["format", "index", "manifest", "path", "signature"];
/// The largest integer a JavaScript number holds exactly: the contract's
/// integers are read as the reference implementation reads them.
const MAX_SAFE_INTEGER: i128 = (1 << 53) - 1;

const MAINNET_VERSIONS: [u32; 5] = [0x0488b21e, 0x049d7cb2, 0x04b24746, 0x0295b43f, 0x02aa7ed3];
const TESTNET_VERSIONS: [u32; 5] = [0x043587cf, 0x044a5262, 0x045f1cf6, 0x024289ef, 0x02575483];

/// Why a proof is invalid. The first failing check decides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProofCode {
    Format,
    Manifest,
    Purpose,
    Network,
    Seed,
    KeyShape,
    AccountRange,
    Index,
    Path,
    Signature,
    WrongKey,
}

impl ProofCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Format => "PROOF_FORMAT",
            Self::Manifest => "PROOF_MANIFEST",
            Self::Purpose => "PROOF_PURPOSE",
            Self::Network => "PROOF_NETWORK",
            Self::Seed => "PROOF_SEED",
            Self::KeyShape => "PROOF_KEY_SHAPE",
            Self::AccountRange => "PROOF_ACCOUNT_RANGE",
            Self::Index => "PROOF_INDEX",
            Self::Path => "PROOF_PATH",
            Self::Signature => "PROOF_SIGNATURE",
            Self::WrongKey => "PROOF_WRONG_KEY",
        }
    }
}

/// The result of checking one key's proof.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProofStatus {
    Verified,
    Unpinned,
    Missing,
    Invalid(ProofCode),
}

impl fmt::Display for ProofStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Verified => write!(f, "verified"),
            Self::Unpinned => write!(f, "unpinned"),
            Self::Missing => write!(f, "missing"),
            Self::Invalid(code) => write!(f, "invalid ({})", code.as_str()),
        }
    }
}

/// A key as the backend serves it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvenKey<'a> {
    pub fingerprint: &'a str,
    pub origin_path: &'a str,
    pub xpub: &'a str,
}

fn is_lower_hex(s: &str, len: usize) -> bool {
    s.len() == len && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// `0` or up to ten digits without a leading zero.
fn parse_dec(s: &str) -> Option<u64> {
    let bytes = s.as_bytes();
    if bytes.is_empty() || bytes.len() > 10 || !bytes.iter().all(u8::is_ascii_digit) {
        return None;
    }
    if bytes.len() > 1 && bytes[0] == b'0' {
        return None;
    }
    s.parse().ok()
}

fn is_xpub_text(s: &str) -> bool {
    // Base58 only, at most 120 characters: nothing that could change the
    // shape of a leaf.
    !s.is_empty()
        && s.len() <= 120
        && s.bytes().all(|b| {
            matches!(b, b'1'..=b'9' | b'A'..=b'H' | b'J'..=b'N' | b'P'..=b'Z' | b'a'..=b'k' | b'm'..=b'z')
        })
}

/// The canonical origin `48'/<coin>'/<account>'/2'`.
fn parse_origin(s: &str) -> Option<(u64, u64)> {
    let rest = s.strip_prefix("48'/")?;
    let rest = rest.strip_suffix("'/2'")?;
    let (coin, account) = rest.split_once("'/")?;
    Some((parse_dec(coin)?, parse_dec(account)?))
}

/// One key's leaf: seven fixed lines, joined by "\n", no trailing newline.
pub fn leaf_bytes(
    account: u64,
    fingerprint: &str,
    xpub: &str,
    purpose: Purpose,
    coin: u64,
) -> Vec<u8> {
    [
        "21C pool leaf v2".to_string(),
        format!("purpose {}", purpose.as_str()),
        format!("chain 48'/{coin}'"),
        format!("account {account}"),
        format!("seed {}", fingerprint.to_lowercase()),
        format!("path 48'/{coin}'/{account}'/2'"),
        format!("xpub {xpub}"),
    ]
    .join("\n")
    .into_bytes()
}

/// RFC 9162: leaf = SHA-256(0x00 || leaf), node = SHA-256(0x01 || L || R).
pub fn leaf_hash(
    account: u64,
    fingerprint: &str,
    xpub: &str,
    purpose: Purpose,
    coin: u64,
) -> [u8; 32] {
    let mut data = vec![0u8];
    data.extend(leaf_bytes(account, fingerprint, xpub, purpose, coin));
    sha256::Hash::hash(&data).to_byte_array()
}

fn node_hash(left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
    let mut data = Vec::with_capacity(65);
    data.push(1u8);
    data.extend_from_slice(left);
    data.extend_from_slice(right);
    sha256::Hash::hash(&data).to_byte_array()
}

fn equal_bytes(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |diff, (x, y)| diff | (x ^ y)) == 0
}

/// RFC 9162 section 2.1.3.2: is `leaf` at `index` of a tree of `size` under `root`?
fn verify_audit_path(
    leaf: [u8; 32],
    index: u64,
    size: u64,
    path: &[[u8; 32]],
    root: &[u8; 32],
) -> bool {
    if index >= size || path.len() > MAX_PROOF_PATH {
        return false;
    }
    let mut fn_ = index;
    let mut sn = size - 1;
    let mut r = leaf;
    for p in path {
        if sn == 0 {
            return false;
        }
        if fn_ % 2 == 1 || fn_ == sn {
            r = node_hash(p, &r);
            if fn_ % 2 == 0 {
                while fn_ % 2 == 0 && fn_ != 0 {
                    fn_ /= 2;
                    sn /= 2;
                }
            }
        } else {
            r = node_hash(&r, p);
        }
        fn_ /= 2;
        sn /= 2;
    }
    sn == 0 && equal_bytes(&r, root)
}

/// The root of a tree over `leaves`: a lone last node moves up a level
/// unchanged (RFC 9162's MTH). Used to check the golden trees.
#[cfg(test)]
fn merkle_root(leaves: &[[u8; 32]]) -> [u8; 32] {
    let mut level = leaves.to_vec();
    while level.len() > 1 {
        level = level
            .chunks(2)
            .map(|pair| match pair {
                [left, right] => node_hash(left, right),
                [lone] => *lone,
                _ => unreachable!(),
            })
            .collect();
    }
    level[0]
}

/// The strict v2 manifest.
struct Manifest {
    purpose: Purpose,
    seed: String,
    coin: u64,
    first: u64,
    count: u64,
    last: u64,
    root: [u8; 32],
}

/// The strict v2 manifest (ASCII, seven lines in order, no trailing newline,
/// decimals without leading zeros, lowercase hex, count == last - first + 1,
/// last <= 2^31 - 1), else `None`. The chain's coin is read as written: a coin
/// that is not the vault's is the caller's PROOF_NETWORK.
fn parse_manifest(text: &str) -> Option<Manifest> {
    let lines: Vec<&str> = text.split('\n').collect();
    let [head, purpose, seed, chain, accounts, keys, root] = lines.as_slice() else {
        return None;
    };
    if *head != "21C key pool v2" {
        return None;
    }
    let purpose = Purpose::from_manifest(purpose.strip_prefix("purpose ")?)?;
    let seed = seed.strip_prefix("seed ")?;
    if !is_lower_hex(seed, 8) {
        return None;
    }
    let coin = parse_dec(chain.strip_prefix("chain 48'/")?.strip_suffix('\'')?)?;
    let (first, last) = accounts.strip_prefix("accounts ")?.split_once('-')?;
    let (first, last) = (parse_dec(first)?, parse_dec(last)?);
    let count = parse_dec(keys.strip_prefix("keys ")?)?;
    let root = root.strip_prefix("root ")?;
    if !is_lower_hex(root, 64) {
        return None;
    }
    if first > last || last > MAX_ACCOUNT || count != last - first + 1 {
        return None;
    }
    let mut root_bytes = [0u8; 32];
    hex::decode_to_slice(root, &mut root_bytes).ok()?;
    Some(Manifest {
        purpose,
        seed: seed.to_string(),
        coin,
        first,
        count,
        last,
        root: root_bytes,
    })
}

/// The compressed public key behind a BIP-137 signature (base64, 65 bytes:
/// header 27-42, r, s). A high-S signature is refused.
fn recover_signer(message: &str, signature_b64: &str) -> Option<[u8; 33]> {
    let sig = STANDARD.decode(signature_b64).ok()?;
    if sig.len() != 65 {
        return None;
    }
    let header = sig[0];
    if !(27..=42).contains(&header) {
        return None;
    }
    let recovery = RecoveryId::from_i32(((header - 27) & 3) as i32).ok()?;
    let recoverable = RecoverableSignature::from_compact(&sig[1..], recovery).ok()?;
    let standard = recoverable.to_standard();
    let mut normalized = standard;
    normalized.normalize_s();
    if normalized != standard {
        return None;
    }
    let digest = signed_msg_hash(message).to_byte_array();
    let pubkey = Secp256k1::verification_only()
        .recover_ecdsa(&Message::from_digest(digest), &recoverable)
        .ok()?;
    Some(pubkey.serialize())
}

fn p2pkh_address(pubkey: &[u8], coin: u64) -> String {
    let version = if coin == 0 { 0x00 } else { 0x6f };
    let mut data = vec![version];
    data.extend_from_slice(hash160::Hash::hash(pubkey).as_byte_array());
    base58::encode_check(&data)
}

/// A pin counts only as a P2PKH address of its own network.
fn is_pin_address(pin: &str, coin: u64) -> bool {
    let pin = pin.trim();
    if pin.is_empty() {
        return false;
    }
    match base58::decode_check(pin) {
        Ok(raw) => raw.len() == 21 && raw[0] == if coin == 0 { 0x00 } else { 0x6f },
        Err(_) => false,
    }
}

/// The pinned address for (network, purpose, seed): the seed is matched in
/// lower case, and a pin that is not a P2PKH address of that network reads as
/// no pin at all.
fn pin_for<'a>(
    pins: &'a [Pin],
    network: ProofNet,
    purpose: Purpose,
    seed: &str,
) -> Option<&'a str> {
    pins.iter()
        .find(|pin| {
            pin.network == network
                && pin.purpose == purpose
                && pin.seed.to_lowercase() == seed
                && is_pin_address(&pin.address, network.coin())
        })
        .map(|pin| pin.address.trim())
}

/// A JSON number read as the reference implementation reads it: an integer
/// when it has no fraction, whatever way it was written.
fn json_integer(value: &Value) -> Option<i128> {
    let Value::Number(n) = value else {
        return None;
    };
    if let Some(i) = n.as_i64() {
        return Some(i as i128);
    }
    if let Some(u) = n.as_u64() {
        return Some(u as i128);
    }
    let f = n.as_f64()?;
    (f.is_finite() && f.fract() == 0.0 && f.abs() < 1e30).then_some(f as i128)
}

/// The seven steps of the contract, in the reference order; the first failure
/// decides the code. `account` is the account the server named for this vault
/// (optional; the key's origin path decides and the two must agree). `proof`
/// is the raw `proof` field: absent or null is missing.
pub fn check_key_proof(
    key: &ProvenKey,
    proof: Option<&Value>,
    network: ProofNet,
    purpose: Purpose,
    account: Option<u64>,
    pins: &[Pin],
) -> ProofStatus {
    use ProofCode::*;
    let invalid = ProofStatus::Invalid;
    let coin = network.coin();
    let proof = match proof {
        None | Some(Value::Null) => return ProofStatus::Missing,
        Some(p) => p,
    };

    // 1. What the proof must be about comes from the key and the slot.
    let Some((key_coin, key_account)) = parse_origin(key.origin_path) else {
        return invalid(KeyShape);
    };
    if key_coin != coin {
        return invalid(Network);
    }
    if key_account > MAX_ACCOUNT {
        return invalid(KeyShape);
    }
    if account.is_some_and(|a| a != key_account) {
        return invalid(KeyShape);
    }
    let fingerprint = key.fingerprint.to_lowercase();
    if !is_lower_hex(&fingerprint, 8) {
        return invalid(KeyShape);
    }
    if !is_xpub_text(key.xpub) {
        return invalid(KeyShape);
    }

    // 2. Format (exactly the five fields, their types) and the strict manifest.
    let Value::Object(p) = proof else {
        return invalid(Format);
    };
    let mut fields: Vec<&str> = p.keys().map(String::as_str).collect();
    fields.sort_unstable();
    if fields != PROOF_FIELDS {
        return invalid(Format);
    }
    if p["format"].as_str() != Some(PROOF_FORMAT) {
        return invalid(Format);
    }
    let (Value::String(manifest_text), Value::String(signature), Value::Array(raw_path)) =
        (&p["manifest"], &p["signature"], &p["path"])
    else {
        return invalid(Format);
    };
    let Some(index) = json_integer(&p["index"]) else {
        return invalid(Format);
    };
    let Some(manifest) = parse_manifest(manifest_text) else {
        return invalid(Manifest);
    };

    // 3. Purpose, chain and seed.
    if manifest.purpose != purpose {
        return invalid(Purpose);
    }
    if manifest.coin != coin {
        return invalid(Network);
    }
    if manifest.seed != fingerprint {
        return invalid(Seed);
    }

    // 4. The key's place in the signed range.
    if key_account < manifest.first || key_account > manifest.last {
        return invalid(AccountRange);
    }
    if index != (key_account - manifest.first) as i128 {
        return invalid(Index);
    }

    // 5. Inclusion under the signed root.
    if raw_path.len() > MAX_PROOF_PATH {
        return invalid(Path);
    }
    let mut path = Vec::with_capacity(raw_path.len());
    for hash in raw_path {
        let Value::String(hash) = hash else {
            return invalid(Path);
        };
        if !is_lower_hex(hash, 64) {
            return invalid(Path);
        }
        let mut bytes = [0u8; 32];
        if hex::decode_to_slice(hash, &mut bytes).is_err() {
            return invalid(Path);
        }
        path.push(bytes);
    }
    let leaf = leaf_hash(key_account, &fingerprint, key.xpub, purpose, coin);
    if !(0..=MAX_SAFE_INTEGER).contains(&index)
        || !verify_audit_path(leaf, index as u64, manifest.count, &path, &manifest.root)
    {
        return invalid(Path);
    }

    // 6. Who signed the manifest.
    let Some(signer) = recover_signer(manifest_text, signature) else {
        return invalid(Signature);
    };
    let address = p2pkh_address(&signer, coin);

    // 7. Is that the device pinned for this seed?
    match pin_for(pins, network, purpose, &fingerprint) {
        None => ProofStatus::Unpinned,
        Some(pin) if address != pin => invalid(WrongKey),
        Some(_) => ProofStatus::Verified,
    }
}

/// A 21st Capital key as the exchange answer carries it.
#[derive(Debug, Clone, PartialEq)]
pub struct IssuedKey {
    pub fingerprint: String,
    pub origin_path: String,
    pub xpub: String,
    pub proof: Option<Value>,
}

impl IssuedKey {
    fn proven(&self) -> ProvenKey<'_> {
        ProvenKey {
            fingerprint: &self.fingerprint,
            origin_path: &self.origin_path,
            xpub: &self.xpub,
        }
    }
}

/// The checks the proof cannot replace: a well-formed key of the network at
/// depth four on the multisig purpose, and on Bitcoin the role's live seed.
fn key_problem(key: &IssuedKey, network: ProofNet, purpose: Purpose) -> Option<String> {
    let fingerprint = key.fingerprint.to_lowercase();
    if !is_lower_hex(&fingerprint, 8) {
        return Some("unreadable fingerprint".to_string());
    }
    let Ok(bytes) = base58::decode_check(key.xpub.trim()) else {
        return Some("unreadable key".to_string());
    };
    if bytes.len() != 78 {
        return Some("unreadable key".to_string());
    }
    let version = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    let versions = match network {
        ProofNet::Mainnet => &MAINNET_VERSIONS,
        ProofNet::Testnet4 => &TESTNET_VERSIONS,
    };
    if !versions.contains(&version) {
        return Some("key of another network".to_string());
    }
    if bytes[4] != 4 {
        return Some("key at the wrong depth".to_string());
    }
    if !key.origin_path.is_empty() {
        match lenient_origin_coin(key.origin_path.trim()) {
            None => return Some("key for another purpose".to_string()),
            Some(coin) if coin != network.coin() => {
                return Some("key of another network".to_string())
            }
            Some(_) => {}
        }
    }
    if network == ProofNet::Mainnet {
        let pinned = match purpose {
            Purpose::Cosigner => PINNED_MAINNET_COSIGNER,
            Purpose::Rescue => PINNED_MAINNET_RESCUE,
        };
        if !pinned.contains(&fingerprint.as_str()) {
            let role = match purpose {
                Purpose::Cosigner => "co-signing",
                Purpose::Rescue => "rescue",
            };
            return Some(format!("unknown {role} seed {fingerprint}"));
        }
    }
    None
}

/// The coin of `[m/]48'/<coin>'/<account>'/2'`, with `'` or `h` (any case).
fn lenient_origin_coin(origin: &str) -> Option<u64> {
    let lower = origin.to_lowercase();
    let path = lower.strip_prefix("m/").unwrap_or(&lower);
    let steps: Vec<&str> = path.split('/').collect();
    let [purpose, coin, account, script] = steps.as_slice() else {
        return None;
    };
    let hardened = |step: &str| -> Option<u64> {
        let digits = step.strip_suffix('\'').or_else(|| step.strip_suffix('h'))?;
        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        digits.parse().ok()
    };
    if hardened(purpose)? != 48 || hardened(script)? != 2 {
        return None;
    }
    hardened(account)?;
    hardened(coin)
}

/// `None` when the issued keys may go into a new vault, else the reason (for
/// the log, never shown as the words of the screen; it carries no xpub).
/// `account` is the account the backend named for this vault: each key's
/// proof must be for that account.
pub fn issued_keys_problem(
    cosigner: Option<&IssuedKey>,
    rescue: Option<&IssuedKey>,
    network: ProofNet,
    account: Option<u64>,
    pins: &[Pin],
) -> Option<String> {
    for (key, purpose) in [(cosigner, Purpose::Cosigner), (rescue, Purpose::Rescue)] {
        if let Some(key) = key {
            if let Some(problem) = key_problem(key, network, purpose) {
                return Some(format!("{}: {problem}", purpose.label()));
            }
        }
    }
    if let (Some(cosigner), Some(rescue)) = (cosigner, rescue) {
        // Always two seeds on two devices: one key must never be both.
        if cosigner.fingerprint.to_lowercase() == rescue.fingerprint.to_lowercase() {
            return Some("the co-signing and rescue keys come from the same seed".to_string());
        }
        if cosigner.xpub.trim() == rescue.xpub.trim() {
            return Some("the co-signing and rescue keys are the same key".to_string());
        }
    }
    // Enforced and required on both networks: anything but a verified proof
    // refuses. Both keys are judged before the first refusal is returned, so
    // both reach the log.
    let mut refusal = None;
    for (key, purpose) in [(cosigner, Purpose::Cosigner), (rescue, Purpose::Rescue)] {
        let Some(key) = key else { continue };
        let status = check_key_proof(
            &key.proven(),
            key.proof.as_ref(),
            network,
            purpose,
            account,
            pins,
        );
        if status != ProofStatus::Verified {
            tracing::warn!("{} key proof {}", purpose.as_str(), status);
            refusal.get_or_insert_with(|| format!("{}: proof {}", purpose.label(), status));
        }
    }
    refusal
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHARED: &str = include_str!("testdata/key-proof-vectors.json");
    const TALIA: &str = include_str!("testdata/key-proof-vectors.talia.json");

    fn net(s: &Value) -> ProofNet {
        ProofNet::from_contract(s.as_str().unwrap()).unwrap()
    }

    fn purpose(s: &Value) -> Purpose {
        Purpose::from_manifest(s.as_str().unwrap()).unwrap()
    }

    fn expected(case: &Value) -> ProofStatus {
        let code = |s: &str| match s {
            "PROOF_FORMAT" => ProofCode::Format,
            "PROOF_MANIFEST" => ProofCode::Manifest,
            "PROOF_PURPOSE" => ProofCode::Purpose,
            "PROOF_NETWORK" => ProofCode::Network,
            "PROOF_SEED" => ProofCode::Seed,
            "PROOF_KEY_SHAPE" => ProofCode::KeyShape,
            "PROOF_ACCOUNT_RANGE" => ProofCode::AccountRange,
            "PROOF_INDEX" => ProofCode::Index,
            "PROOF_PATH" => ProofCode::Path,
            "PROOF_SIGNATURE" => ProofCode::Signature,
            "PROOF_WRONG_KEY" => ProofCode::WrongKey,
            other => panic!("unknown code {}", other),
        };
        match case["expect"]["status"].as_str().unwrap() {
            "verified" => ProofStatus::Verified,
            "unpinned" => ProofStatus::Unpinned,
            "missing" => ProofStatus::Missing,
            "invalid" => ProofStatus::Invalid(code(case["expect"]["code"].as_str().unwrap())),
            other => panic!("unknown status {}", other),
        }
    }

    /// A key field that is not text reads as empty text: both fail the same
    /// checks.
    fn text(v: &Value) -> &str {
        v.as_str().unwrap_or("")
    }

    /// `{network: {purpose: {seed: address}}}`, entries that are not text left out.
    fn full_pins(v: &Value) -> Vec<Pin> {
        let mut pins = Vec::new();
        for (network, n) in [
            ("mainnet", ProofNet::Mainnet),
            ("testnet4", ProofNet::Testnet4),
        ] {
            for (purpose, p) in [("cosigner", Purpose::Cosigner), ("rescue", Purpose::Rescue)] {
                if let Some(table) = v[network][purpose].as_object() {
                    pins.extend(table.iter().filter_map(|(seed, address)| {
                        Some(Pin {
                            network: n,
                            purpose: p,
                            seed: seed.clone(),
                            address: address.as_str()?.to_string(),
                        })
                    }));
                }
            }
        }
        pins
    }

    fn run(case: &Value, account: Option<u64>, pins: &[Pin]) {
        let key = &case["key"];
        let status = check_key_proof(
            &ProvenKey {
                fingerprint: text(&key["fingerprint"]),
                origin_path: text(&key["origin_path"]),
                xpub: text(&key["xpub"]),
            },
            case.get("proof"),
            net(&case["network"]),
            purpose(&case["purpose"]),
            account,
            pins,
        );
        assert_eq!(status, expected(case), "{}", case["name"]);
    }

    #[test]
    fn the_shared_golden_vectors() {
        let vectors: Value = serde_json::from_str(SHARED).unwrap();
        assert_eq!(vectors["proof_format"], PROOF_FORMAT);
        let cases = vectors["cases"].as_array().unwrap();
        assert!(cases.len() > 50);
        for case in cases {
            // The shared set pins one (network, purpose) table per case.
            let pins: Vec<Pin> = case["pins"]
                .as_object()
                .unwrap()
                .iter()
                .filter_map(|(seed, address)| {
                    Some(Pin {
                        network: net(&case["network"]),
                        purpose: purpose(&case["purpose"]),
                        seed: seed.clone(),
                        address: address.as_str()?.to_string(),
                    })
                })
                .collect();
            run(case, case["key"]["account"].as_u64(), &pins);
        }
    }

    #[test]
    fn the_shared_golden_trees() {
        let vectors: Value = serde_json::from_str(SHARED).unwrap();
        for tree in vectors["trees"].as_array().unwrap() {
            let coin = net(&tree["network"]).coin();
            let p = purpose(&tree["purpose"]);
            let leaves: Vec<[u8; 32]> = tree["rows"]
                .as_array()
                .unwrap()
                .iter()
                .zip(tree["leaves"].as_array().unwrap())
                .map(|(row, leaf)| {
                    let account = row["account"].as_u64().unwrap();
                    let fp = row["fingerprint"].as_str().unwrap();
                    let xpub = row["xpub"].as_str().unwrap();
                    assert_eq!(
                        String::from_utf8(leaf_bytes(account, fp, xpub, p, coin)).unwrap(),
                        leaf["text"].as_str().unwrap()
                    );
                    let hash = leaf_hash(account, fp, xpub, p, coin);
                    assert_eq!(hex::encode(hash), leaf["hash"].as_str().unwrap());
                    hash
                })
                .collect();
            assert_eq!(
                hex::encode(merkle_root(&leaves)),
                tree["root"].as_str().unwrap()
            );
            let manifest = parse_manifest(tree["manifest"].as_str().unwrap()).unwrap();
            assert_eq!(manifest.count, tree["count"].as_u64().unwrap());
        }
    }

    #[test]
    fn the_talia_golden_vectors() {
        let vectors: Value = serde_json::from_str(TALIA).unwrap();
        assert_eq!(vectors["format"], "21c-key-proof-vectors-v1");
        let default = full_pins(&vectors["pins"]);
        let cases = vectors["cases"].as_array().unwrap();
        assert!(cases.len() > 70);
        for case in cases {
            let pins = if case["pins"] == "default" {
                default.clone()
            } else {
                full_pins(&case["pins"])
            };
            run(case, case["account"].as_u64(), &pins);
        }
    }

    #[test]
    fn the_talia_golden_leaf_and_roots() {
        let vectors: Value = serde_json::from_str(TALIA).unwrap();
        let example = &vectors["leaf_example"];
        let row = &example["row"];
        let bytes = leaf_bytes(
            row["account"].as_u64().unwrap(),
            row["fingerprint"].as_str().unwrap(),
            row["xpub"].as_str().unwrap(),
            purpose(&example["purpose"]),
            example["coin"].as_u64().unwrap(),
        );
        assert_eq!(hex::encode(&bytes), example["bytes_hex"].as_str().unwrap());

        // The tools' placeholder xpubs: the account number in base58 letters.
        fn placeholder(coin: u64, purpose: &str, account: u64) -> String {
            const A: &[u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
            let mut digits = Vec::new();
            let mut n = account;
            loop {
                digits.push(A[(n % 58) as usize] as char);
                n /= 58;
                if n == 0 {
                    break;
                }
            }
            let digits: String = digits.into_iter().rev().collect();
            let mut role = purpose.to_string();
            role[..1].make_ascii_uppercase();
            let prefix = if coin == 0 { "xpub" } else { "tpub" };
            format!("{prefix}TestVector{role}Key{digits}")
        }
        let rows = &vectors["merkle_rows"];
        let (seed, p, coin, first) = (
            rows["seed"].as_str().unwrap(),
            rows["purpose"].as_str().unwrap(),
            rows["coin"].as_u64().unwrap(),
            rows["first"].as_u64().unwrap(),
        );
        for entry in vectors["merkle"].as_array().unwrap() {
            let count = entry["count"].as_u64().unwrap();
            let leaves: Vec<[u8; 32]> = (0..count)
                .map(|i| {
                    let account = first + i;
                    leaf_hash(
                        account,
                        seed,
                        &placeholder(coin, p, account),
                        Purpose::from_manifest(p).unwrap(),
                        coin,
                    )
                })
                .collect();
            assert_eq!(
                hex::encode(merkle_root(&leaves)),
                entry["root"].as_str().unwrap()
            );
        }
    }

    #[test]
    fn every_inclusion_proof_checks_out_and_nothing_else() {
        for n in 1..=20u64 {
            let leaves: Vec<[u8; 32]> = (0..n)
                .map(|i| sha256::Hash::hash(format!("leaf {i}").as_bytes()).to_byte_array())
                .collect();
            let root = merkle_root(&leaves);
            for i in 0..n {
                let path = audit_path(&leaves, i as usize);
                assert!(verify_audit_path(leaves[i as usize], i, n, &path, &root));
                // The same path proves no other position, and no other leaf.
                for j in (0..n).filter(|j| *j != i) {
                    assert!(!verify_audit_path(leaves[i as usize], j, n, &path, &root));
                }
                assert!(!verify_audit_path([7u8; 32], i, n, &path, &root));
            }
        }
    }

    /// Every leaf's audit path, bottom-up (the reference's `merkleTree`).
    fn audit_path(leaves: &[[u8; 32]], index: usize) -> Vec<[u8; 32]> {
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
                        next.push((node_hash(l, r), [lm.clone(), rm.clone()].concat()));
                    }
                    [lone] => next.push(lone.clone()),
                    _ => unreachable!(),
                }
            }
            level = next;
        }
        path
    }

    #[test]
    fn the_default_pins_are_addresses_of_their_network() {
        for pin in default_pins() {
            assert!(
                is_pin_address(&pin.address, pin.network.coin()),
                "{}",
                pin.seed
            );
        }
    }

    /// A depth-4 mainnet xpub at 48'/0'/1003'/2' of a throwaway test seed.
    fn mainnet_xpub() -> String {
        use liana::miniscript::bitcoin::{
            bip32::{DerivationPath, Xpriv, Xpub},
            NetworkKind,
        };
        use std::str::FromStr;
        let secp = Secp256k1::new();
        let master = Xpriv::new_master(NetworkKind::Main, &[7u8; 32]).unwrap();
        let path = DerivationPath::from_str("m/48'/0'/1003'/2'").unwrap();
        let xpriv = master.derive_priv(&secp, &path).unwrap();
        Xpub::from_priv(&secp, &xpriv).to_string()
    }

    #[test]
    fn issued_keys_refuse_unknown_mainnet_seeds_before_any_proof() {
        let key = IssuedKey {
            fingerprint: "a1b2c3d4".to_string(),
            origin_path: "48'/0'/1003'/2'".to_string(),
            xpub: mainnet_xpub(),
            proof: None,
        };
        let problem = issued_keys_problem(
            Some(&key),
            None,
            ProofNet::Mainnet,
            Some(1003),
            &default_pins(),
        )
        .unwrap();
        assert!(problem.contains("unknown co-signing seed"), "{}", problem);
        // On Testnet4 the same key is a key of another network.
        let problem = issued_keys_problem(
            Some(&key),
            None,
            ProofNet::Testnet4,
            Some(1003),
            &default_pins(),
        )
        .unwrap();
        assert!(problem.contains("another network"), "{}", problem);
    }

    #[test]
    fn issued_keys_without_a_proof_are_refused() {
        let key = IssuedKey {
            fingerprint: "bb09d891".to_string(),
            origin_path: "48'/0'/1003'/2'".to_string(),
            xpub: mainnet_xpub(),
            proof: None,
        };
        let problem = issued_keys_problem(
            Some(&key),
            None,
            ProofNet::Mainnet,
            Some(1003),
            &default_pins(),
        )
        .unwrap();
        assert_eq!(problem, "co-signing key: proof missing");
        let rescue = IssuedKey {
            fingerprint: "bb09d891".to_string(),
            ..key.clone()
        };
        assert_eq!(
            issued_keys_problem(Some(&key), Some(&rescue), ProofNet::Mainnet, None, &[]).unwrap(),
            "rescue key: unknown rescue seed bb09d891"
        );
    }

    #[test]
    fn lenient_origin_reads_both_hardened_markers() {
        assert_eq!(lenient_origin_coin("48'/1'/7'/2'"), Some(1));
        assert_eq!(lenient_origin_coin("m/48h/0h/7h/2h"), Some(0));
        assert_eq!(lenient_origin_coin("M/48H/0H/7H/2H"), Some(0));
        assert_eq!(lenient_origin_coin("84'/0'/0'"), None);
        assert_eq!(lenient_origin_coin("48'/0'/0'/1'"), None);
        assert_eq!(lenient_origin_coin("48/0'/0'/2'"), None);
    }
}

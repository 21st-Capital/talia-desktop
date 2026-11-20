//! Talia: the Send screen's Slow / Medium / High fee presets from mempool.space,
//! as Talia on the iPhone shows them (decided 2026-11-19).
//!
//! Liana only fills these presets from its own Liana Connect service, so a wallet
//! on the user's own node got an empty feerate box. This asks mempool.space's
//! precise recommendations (`api/v1/fees/precise`, the figures its website shows
//! under "Transaction fees"): High = next block, Medium = about 30 minutes,
//! Slow = about an hour. The request carries no wallet data.
//!
//! Liana's engine takes whole sats/vbyte from 1 to `MAX_FEERATE_VB`, so each rate
//! is first written the way mempool.space's website writes it (one decimal under
//! 10) and then rounded UP to a whole number: a preset never pays less than the
//! site shows. Nothing here changes how a transaction is built or checked; the
//! number only pre-fills the feerate box, which Liana validates as before.

use std::time::Duration;

use liana::{
    miniscript::bitcoin::Network,
    spend::{MAX_FEERATE_VB, MIN_FEERATE_VB},
};

use crate::daemon::FeerateEstimate;

const TIMEOUT: Duration = Duration::from_secs(6);

fn base_url(network: Network) -> Option<&'static str> {
    match network {
        Network::Bitcoin => Some("https://mempool.space/api/v1/fees"),
        Network::Testnet4 => Some("https://mempool.space/testnet4/api/v1/fees"),
        Network::Signet => Some("https://mempool.space/signet/api/v1/fees"),
        Network::Testnet => Some("https://mempool.space/testnet/api/v1/fees"),
        _ => None,
    }
}

/// Whether presets exist for this network (none on regtest).
pub fn supported(network: Network) -> bool {
    base_url(network).is_some()
}

/// A mempool.space rate as a whole sats/vbyte the engine accepts, or `None` when
/// the value is not a usable positive number.
fn whole_sats(rate: &serde_json::Value) -> Option<i32> {
    let rate = rate.as_f64()?;
    if !rate.is_finite() || rate <= 0.0 {
        return None;
    }
    // As mempool.space's website writes it: one decimal under 10 sat/vB.
    let shown = if rate >= 10.0 {
        rate.round()
    } else {
        (rate * 10.0).round() / 10.0
    };
    let whole = shown
        .ceil()
        .clamp(MIN_FEERATE_VB as f64, MAX_FEERATE_VB as f64);
    Some(whole as i32)
}

/// The presets from a `fees/precise` (or `fees/recommended`) answer. Exported for tests.
pub fn estimate_from_json(json: &serde_json::Value) -> Option<FeerateEstimate> {
    let high = whole_sats(json.get("fastestFee")?)?;
    let medium = whole_sats(json.get("halfHourFee")?)?;
    let slow = whole_sats(json.get("hourFee")?)?;
    // `FeerateEstimate::new` floors each preset at the minimum and keeps
    // slow <= medium <= high.
    Some(FeerateEstimate::new(slow, Some(medium), high))
}

async fn get(client: &reqwest::Client, url: &str) -> Option<FeerateEstimate> {
    let response = client.get(url).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    let json: serde_json::Value = response.json().await.ok()?;
    estimate_from_json(&json)
}

/// Fetches the presets: the precise figures first, mempool.space's rounded
/// endpoint second. `None` when neither answers; the feerate box then stays
/// empty for the user to fill, as in Liana.
pub async fn fetch(network: Network) -> Option<FeerateEstimate> {
    let base = base_url(network)?;
    let client = reqwest::Client::builder().timeout(TIMEOUT).build().ok()?;
    if let Some(est) = get(&client, &format!("{base}/precise")).await {
        return Some(est);
    }
    get(&client, &format!("{base}/recommended")).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rounds_up_from_the_figure_the_site_shows() {
        // Sampled 2026-11-19: the site showed 1.5 / 0.8 / 0.3.
        let est = estimate_from_json(&json!({
            "fastestFee": 1.501, "halfHourFee": 0.812, "hourFee": 0.342,
            "economyFee": 0.2, "minimumFee": 0.1
        }))
        .unwrap();
        assert_eq!((est.low, est.medium, est.high), (1, Some(1), 2));

        // 1.002 is "1" on the site, so it stays 1 (not 2).
        let est = estimate_from_json(&json!({"fastestFee": 1.002, "halfHourFee": 1, "hourFee": 1}))
            .unwrap();
        assert_eq!(est.high, 1);

        let est =
            estimate_from_json(&json!({"fastestFee": 12.4, "halfHourFee": 8.04, "hourFee": 3.26}))
                .unwrap();
        // 8.04 shows as "8" on the site, 3.26 as "3.3".
        assert_eq!((est.low, est.medium, est.high), (4, Some(8), 12));
    }

    #[test]
    fn keeps_the_order_and_the_engine_limits() {
        let est =
            estimate_from_json(&json!({"fastestFee": 2, "halfHourFee": 5, "hourFee": 9})).unwrap();
        assert_eq!((est.low, est.medium, est.high), (9, Some(9), 9));

        // A broken or hostile answer can never preset above Liana's maximum.
        let est = estimate_from_json(
            &json!({"fastestFee": 20000, "halfHourFee": 15000, "hourFee": 5000}),
        )
        .unwrap();
        assert_eq!(est.high, MAX_FEERATE_VB);
        assert_eq!(est.low, MAX_FEERATE_VB);
    }

    #[test]
    fn refuses_unusable_answers() {
        assert!(estimate_from_json(&json!({})).is_none());
        assert!(estimate_from_json(&json!({"fastestFee": 1, "halfHourFee": 1})).is_none());
        assert!(
            estimate_from_json(&json!({"fastestFee": "1", "halfHourFee": 1, "hourFee": 1}))
                .is_none()
        );
        assert!(
            estimate_from_json(&json!({"fastestFee": 0, "halfHourFee": 1, "hourFee": 1})).is_none()
        );
        assert!(
            estimate_from_json(&json!({"fastestFee": -3, "halfHourFee": 1, "hourFee": 1}))
                .is_none()
        );
    }

    /// Asks mempool.space for real: `cargo test -p liana-gui fees -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn live_mempool_space() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        for network in [Network::Bitcoin, Network::Testnet4] {
            let est = fetch(network).await.expect("mempool.space answered");
            assert!(est.low >= MIN_FEERATE_VB && est.high <= MAX_FEERATE_VB);
            assert!(est.low <= est.medium.unwrap() && est.medium.unwrap() <= est.high);
            println!(
                "{network}: slow {} medium {:?} high {}",
                est.low, est.medium, est.high
            );
        }
        assert!(fetch(Network::Regtest).await.is_none());
    }

    #[test]
    fn no_presets_on_regtest() {
        assert!(!supported(Network::Regtest));
        assert!(supported(Network::Bitcoin));
        assert!(supported(Network::Testnet4));
    }
}

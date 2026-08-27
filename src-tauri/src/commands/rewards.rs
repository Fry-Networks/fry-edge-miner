use chrono::Local;
use serde::Serialize;
use std::sync::atomic::Ordering;

const DEFAULT_BASE_REWARD: f64 = 0.0;
const DEFAULT_REWARD_AMOUNT: f64 = 0.0;
const DEFAULT_REWARD_TOKEN_ASA_ID: &str = "";
const DEFAULT_REWARD_TOKEN_NAME: &str = "\u{2014}";
const DEFAULT_STAKE_TOKEN_ASA_ID: &str = "";
const DEFAULT_STAKE_TOKEN_NAME: &str = "\u{2014}";
const SLOTS_PER_DAY: u32 = 144;

#[derive(Debug, Serialize)]
pub struct RewardSummary {
    pub active_count: u32,
    pub total_count: u32,
    pub proportion: f64,
    pub estimated_daily: f64,
    pub base_reward: f64,
    pub reward_amount: f64,
    pub reward_token_asa_id: String,
    pub reward_token_name: String,
    pub stake_token_asa_id: String,
    pub stake_token_name: String,
    pub stake_multiplier: f64,
    pub stake_label: String,
    /// True once the reward config has been resolved at least once (either a
    /// live `/versions/FEM` fetch succeeded, or a prior PoC-loop tick already
    /// cached a positive base reward). False on the very first render after
    /// launch, before the 60s PoC-loop tick has done its first network
    /// round-trip — the frontend must not present `base_reward`/
    /// `estimated_daily` as real numbers while this is false.
    pub config_ready: bool,
    /// True once the verified-stake lookup has resolved, OR the device has no
    /// miner key at all (an unregistered device's 0x multiplier is truthful,
    /// not a cold-cache placeholder). False means `stake_multiplier`/
    /// `stake_label` are best-guess defaults, not confirmed data.
    pub stake_data_ready: bool,
}

/// Pure readiness computation, unit-testable without a `tauri::State`.
///
/// `config_present`  — `cached_reward_config` has resolved at least once.
/// `cached_base`     — the last cached `base_reward` value (0.0 = never warmed).
/// `verified_present` — `cached_verified_status` has resolved at least once.
/// `has_key`         — the device has a saved miner key (i.e. is/was registered).
fn readiness(
    config_present: bool,
    cached_base: f64,
    verified_present: bool,
    has_key: bool,
) -> (bool, bool) {
    let config_ready = config_present || cached_base > 0.0;
    // An unregistered device has no verified-stake call to wait on — its 0x
    // multiplier is already the whole truth, so it's "ready" by definition.
    let stake_data_ready = verified_present || !has_key;
    (config_ready, stake_data_ready)
}

#[cfg(test)]
mod readiness_tests {
    use super::readiness;

    #[test]
    fn cold_cache_registered_device_is_not_ready() {
        assert_eq!(readiness(false, 0.0, false, true), (false, false));
    }

    #[test]
    fn warm_cache_registered_device_is_ready() {
        assert_eq!(readiness(true, 1.5, true, true), (true, true));
    }

    #[test]
    fn unregistered_device_stake_data_is_always_ready() {
        // No miner key → nothing to wait on; the 0x multiplier is truthful.
        let (config_ready, stake_data_ready) = readiness(false, 0.0, false, false);
        assert!(!config_ready);
        assert!(stake_data_ready);
    }

    #[test]
    fn a_previously_cached_positive_base_reward_counts_as_config_ready() {
        // config re-resolves to None on a transient API hiccup, but a prior
        // tick already cached a real number — don't regress to a placeholder.
        let (config_ready, _) = readiness(false, 2.75, false, true);
        assert!(config_ready);
    }
}

#[tauri::command]
pub async fn get_reward_summary(
    state: tauri::State<'_, crate::AppState>,
) -> Result<RewardSummary, String> {
    let registry = state.registry.lock().map_err(|e| e.to_string())?;
    let proportion = registry.proportion();

    let bits = state.cached_base_reward.load(Ordering::Relaxed);
    let cached = f64::from_bits(bits);

    let config = state.cached_reward_config.read().map_err(|e| e.to_string())?;
    let (
        reward_amount,
        reward_token_asa_id,
        reward_token_name,
        stake_token_asa_id,
        stake_token_name,
    ) = config.as_ref().map_or(
        (
            DEFAULT_REWARD_AMOUNT,
            DEFAULT_REWARD_TOKEN_ASA_ID.to_string(),
            DEFAULT_REWARD_TOKEN_NAME.to_string(),
            DEFAULT_STAKE_TOKEN_ASA_ID.to_string(),
            DEFAULT_STAKE_TOKEN_NAME.to_string(),
        ),
        |c| {
            (
                c.reward_amount,
                c.reward_token_asa_id.clone(),
                c.reward_token_name.clone(),
                c.stake_token_asa_id.clone(),
                c.stake_token_name.clone(),
            )
        },
    );

    let base_reward = if config.is_some() {
        reward_amount
    } else if cached > 0.0 {
        cached
    } else {
        DEFAULT_BASE_REWARD
    };

    // Stake multiplier: look up from cached stake_tiers (from /versions/FEM)
    // and cached verified status (from /credentials/{key}/verified)
    let tiers = state.cached_stake_tiers.read().map_err(|e| e.to_string())?;
    let verified = state.cached_verified_status.read().map_err(|e| e.to_string())?;
    let has_key = state.config.get().miner_key.is_some();
    let (config_ready, stake_data_ready) =
        readiness(config.is_some(), cached, verified.is_some(), has_key);

    let (stake_multiplier, stake_label) = match (&*tiers, &*verified) {
        (Some(tiers), Some(vs)) => {
            // /credentials call succeeded → device IS registered.
            // verified = "has verification stake", NOT "is registered".
            // Look up tier from staked.type; default to "none" (registered, no stake = 1×)
            let tier_key = vs.staked.as_ref()
                .and_then(|s| s.stake_type.as_deref())
                .unwrap_or("none");
            tiers.get(tier_key).map_or(
                (1.0, "No stake".to_string()),
                |t| (t.multiplier, t.label.clone()),
            )
        }
        (Some(tiers), None) => {
            // Verified status not yet fetched — check if device is registered
            if has_key {
                tiers.get("none").map_or(
                    (1.0, "No stake".to_string()),
                    |t| (t.multiplier, t.label.clone()),
                )
            } else {
                tiers.get("unregistered").map_or(
                    (0.0, "Not registered".to_string()),
                    |t| (t.multiplier, t.label.clone()),
                )
            }
        }
        _ => {
            // No cached data yet — use registration state as best guess
            if has_key {
                (1.0, "No stake".to_string())
            } else {
                (0.0, "Not registered".to_string())
            }
        }
    };

    Ok(RewardSummary {
        active_count: registry.enabled_count(),
        total_count: registry.total_count(),
        proportion,
        estimated_daily: base_reward * proportion * stake_multiplier,
        base_reward,
        reward_amount,
        reward_token_asa_id,
        reward_token_name,
        stake_token_asa_id,
        stake_token_name,
        stake_multiplier,
        stake_label,
        config_ready,
        stake_data_ready,
    })
}

#[tauri::command]
pub async fn get_poc_slots(
    date: Option<String>,
    state: tauri::State<'_, crate::AppState>,
) -> Result<Vec<crate::poc::PocSlot>, String> {
    let date = date.unwrap_or_else(|| Local::now().format("%Y-%m-%d").to_string());
    let cached = state.poc_cache.load(&date).map_err(|e| e.to_string())?;

    let mut by_index: std::collections::HashMap<u32, crate::poc::PocSlot> =
        std::collections::HashMap::new();
    for slot in cached {
        by_index.insert(
            slot.slot_number,
            crate::poc::PocSlot {
                slot_index: slot.slot_number,
                data: slot.data,
                online: slot.online,
                mac_match: slot.mac_match,
                pol: slot.pol,
                poi: slot.poi,
                poa: slot.poa,
                tools_active: slot.tools_active,
                tools_count: slot.tools_count,
                multiplier: slot.multiplier,
            },
        );
    }

    let mut slots = Vec::with_capacity(SLOTS_PER_DAY as usize);
    for i in 0..SLOTS_PER_DAY {
        slots.push(by_index.remove(&i).unwrap_or(crate::poc::PocSlot {
            slot_index: i,
            data: false,
            online: false,
            mac_match: false,
            pol: false,
            poi: false,
            poa: false,
            tools_active: Vec::new(),
            tools_count: 0,
            multiplier: 0.0,
        }));
    }
    Ok(slots)
}

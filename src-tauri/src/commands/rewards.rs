use chrono::Local;
use serde::Serialize;
use std::sync::atomic::Ordering;

const DEFAULT_BASE_REWARD: f64 = 59.52;
const DEFAULT_REWARD_AMOUNT: f64 = 14.88;
const DEFAULT_REWARD_TOKEN_ASA_ID: &str = "2485202024";
const DEFAULT_REWARD_TOKEN_NAME: &str = "fNODE";
const DEFAULT_STAKE_TOKEN_ASA_ID: &str = "2485314946";
const DEFAULT_STAKE_TOKEN_NAME: &str = "FRY 2.0";
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

    Ok(RewardSummary {
        active_count: registry.enabled_count(),
        total_count: registry.total_count(),
        proportion,
        estimated_daily: base_reward * proportion,
        base_reward,
        reward_amount,
        reward_token_asa_id,
        reward_token_name,
        stake_token_asa_id,
        stake_token_name,
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

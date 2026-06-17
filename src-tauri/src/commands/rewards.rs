use serde::Serialize;
use std::sync::atomic::Ordering;

const DEFAULT_BASE_REWARD: f64 = 59.52;

#[derive(Debug, Serialize)]
pub struct RewardSummary {
    pub active_count: u32,
    pub total_count: u32,
    pub proportion: f64,
    pub estimated_daily: f64,
    pub base_reward: f64,
}

#[tauri::command]
pub async fn get_reward_summary(
    state: tauri::State<'_, crate::AppState>,
) -> Result<RewardSummary, String> {
    let registry = state.registry.lock().map_err(|e| e.to_string())?;
    let proportion = registry.proportion();

    let bits = state.cached_base_reward.load(Ordering::Relaxed);
    let cached = f64::from_bits(bits);
    let base_reward = if cached > 0.0 { cached } else { DEFAULT_BASE_REWARD };

    Ok(RewardSummary {
        active_count: registry.enabled_count(),
        total_count: registry.total_count(),
        proportion,
        estimated_daily: base_reward * proportion,
        base_reward,
    })
}

#[tauri::command]
pub async fn get_poc_slots(
    _date: Option<String>,
    _state: tauri::State<'_, crate::AppState>,
) -> Result<Vec<crate::poc::PocSlot>, String> {
    // Phase 4: fetch historical slot data from local cache or API
    Ok(vec![])
}

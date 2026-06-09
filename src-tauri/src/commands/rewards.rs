use serde::Serialize;

const FEM_BASE_REWARD: f64 = 59.52;

#[derive(Debug, Serialize)]
pub struct RewardSummary {
    pub active_count: u32,
    pub total_count: u32,
    pub proportion: f64,
    pub estimated_daily: f64,
}

#[tauri::command]
pub async fn get_reward_summary(
    state: tauri::State<'_, crate::AppState>,
) -> Result<RewardSummary, String> {
    let registry = state.registry.lock().map_err(|e| e.to_string())?;
    let proportion = registry.proportion();
    Ok(RewardSummary {
        active_count: registry.enabled_count(),
        total_count: registry.total_count(),
        proportion,
        estimated_daily: FEM_BASE_REWARD * proportion,
    })
}

#[tauri::command]
pub async fn get_poc_slots(
    _date: String,
    _state: tauri::State<'_, crate::AppState>,
) -> Result<Vec<crate::poc::PocSlot>, String> {
    // Phase 4: fetch historical slot data from local cache or API
    Ok(vec![])
}

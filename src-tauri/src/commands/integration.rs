use crate::integrations::IntegrationStatus;

#[tauri::command]
pub async fn get_integrations(
    state: tauri::State<'_, crate::AppState>,
) -> Result<Vec<IntegrationStatus>, String> {
    let registry = state.registry.lock().map_err(|e| e.to_string())?;
    Ok(registry.list_statuses())
}

#[tauri::command]
pub async fn toggle_integration(
    id: String,
    enabled: bool,
    state: tauri::State<'_, crate::AppState>,
) -> Result<(), String> {
    let mut registry = state.registry.lock().map_err(|e| e.to_string())?;
    registry.set_enabled(&id, enabled);

    // Update config persistence
    state
        .config
        .update(|cfg| {
            cfg.integrations_enabled.insert(id.clone(), enabled);
        })
        .map_err(|e| e.to_string())?;

    tracing::info!(integration = id, enabled = enabled, "Integration toggled");
    Ok(())
}

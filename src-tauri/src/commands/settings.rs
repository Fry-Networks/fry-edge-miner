use crate::config::FemConfig;

#[tauri::command]
pub async fn get_settings(
    state: tauri::State<'_, crate::AppState>,
) -> Result<FemConfig, String> {
    Ok(state.config.get())
}

#[tauri::command]
pub async fn save_settings(
    settings: FemConfig,
    state: tauri::State<'_, crate::AppState>,
) -> Result<(), String> {
    state
        .config
        .update(|cfg| {
            *cfg = settings;
        })
        .map_err(|e| e.to_string())?;
    tracing::info!("Settings saved");
    Ok(())
}

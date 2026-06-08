use crate::integrations::IntegrationStatus;

#[tauri::command]
pub async fn get_integrations() -> Result<Vec<IntegrationStatus>, String> {
    Ok(vec![])
}

#[tauri::command]
pub async fn toggle_integration(_id: String, _enabled: bool) -> Result<(), String> {
    Ok(())
}

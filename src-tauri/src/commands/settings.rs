#[tauri::command]
pub async fn get_settings() -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({}))
}

#[tauri::command]
pub async fn save_settings(_settings: serde_json::Value) -> Result<(), String> {
    Ok(())
}

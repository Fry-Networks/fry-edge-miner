#[tauri::command]
pub async fn get_device_info() -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({}))
}

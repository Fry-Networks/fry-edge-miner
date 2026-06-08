#[tauri::command]
pub async fn get_rewards() -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({"daily": 0, "weekly": 0, "total": 0}))
}

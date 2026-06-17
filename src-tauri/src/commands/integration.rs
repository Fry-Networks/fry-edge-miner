use crate::integrations::IntegrationStatus;

#[tauri::command]
pub async fn get_integrations(
    state: tauri::State<'_, crate::AppState>,
) -> Result<Vec<IntegrationStatus>, String> {
    let registry = state.registry.lock().map_err(|e| e.to_string())?;
    Ok(registry.list_statuses())
}

#[tauri::command]
pub async fn install_integration(
    id: String,
    state: tauri::State<'_, crate::AppState>,
) -> Result<(), String> {
    // TODO: Registry should store Arc<dyn Integration> to avoid holding lock during install
    tokio::task::block_in_place(|| {
        let reg = state.registry.lock().map_err(|e| e.to_string())?;
        match reg.get(&id) {
            Some(integration) => tokio::runtime::Handle::current()
                .block_on(integration.install())
                .map_err(|e| e.to_string()),
            None => Err(format!("Integration '{}' not found", id)),
        }
    })?;
    tracing::info!(integration = id, "Integration installed");
    Ok(())
}

#[tauri::command]
pub async fn toggle_integration(
    id: String,
    enabled: bool,
    state: tauri::State<'_, crate::AppState>,
) -> Result<(), String> {
    // Start or stop the integration (block_in_place bridges async start/stop
    // through the std::sync::Mutex-guarded registry — same pattern as compute_health_map)
    tokio::task::block_in_place(|| {
        let reg = state.registry.lock().map_err(|e| e.to_string())?;
        match reg.get(&id) {
            Some(integration) => {
                let result = if enabled {
                    tokio::runtime::Handle::current().block_on(integration.start())
                } else {
                    tokio::runtime::Handle::current().block_on(integration.stop())
                };
                result.map_err(|e| e.to_string())
            }
            None => Err(format!("Integration '{}' not found", id)),
        }
    })?;

    // Only update state on success
    {
        let mut reg = state.registry.lock().map_err(|e| e.to_string())?;
        reg.set_enabled(&id, enabled);
    }

    // Persist config
    state
        .config
        .update(|cfg| {
            cfg.integrations_enabled.insert(id.clone(), enabled);
        })
        .map_err(|e| e.to_string())?;

    tracing::info!(integration = id, enabled = enabled, "Integration toggled");
    Ok(())
}

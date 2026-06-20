use crate::integrations::{HealthStatus, IntegrationStatus, LifecycleState};

#[tauri::command]
pub async fn get_integrations(
    state: tauri::State<'_, crate::AppState>,
) -> Result<Vec<IntegrationStatus>, String> {
    // Snapshot registry metadata without holding the lock across any await point.
    let (entries, total) = {
        let reg = state.registry.lock().map_err(|e| e.to_string())?;
        let total = reg.total_count();
        let entries: Vec<(String, String, bool, Option<String>)> = reg
            .list()
            .iter()
            .map(|i| {
                (
                    i.id().to_string(),
                    i.display_name().to_string(),
                    reg.is_enabled(i.id()),
                    i.installed_version(),
                )
            })
            .collect();
        (entries, total)
    };

    // Read the most recent health check results written by the health loop in main.rs.
    let last = state.last_health.read().map_err(|e| e.to_string())?;

    let statuses = entries
        .into_iter()
        .map(|(id, display_name, enabled, version)| {
            let health = if enabled {
                last.get(&id)
                    .cloned()
                    .unwrap_or(HealthStatus::Starting)
            } else {
                last.get(&id)
                    .cloned()
                    .unwrap_or(HealthStatus::Stopped)
            };

            let lifecycle = if !enabled {
                LifecycleState::Disabled
            } else {
                match &health {
                    HealthStatus::Healthy => LifecycleState::Running,
                    HealthStatus::Unhealthy(_) => LifecycleState::Unhealthy,
                    HealthStatus::Installing => LifecycleState::Installing,
                    _ => LifecycleState::Starting,
                }
            };

            IntegrationStatus {
                id,
                display_name,
                enabled,
                health,
                lifecycle,
                version,
                poc_contribution: if enabled { 1.0 / total as f64 } else { 0.0 },
            }
        })
        .collect();

    Ok(statuses)
}

#[tauri::command]
pub async fn install_integration(
    id: String,
    state: tauri::State<'_, crate::AppState>,
) -> Result<(), String> {
    // Clone the integration Arc and release the registry lock before the long install operation.
    let integration = {
        let reg = state.registry.lock().map_err(|e| e.to_string())?;
        reg.get(&id)
            .ok_or_else(|| format!("Integration '{}' not found", id))?
    };

    integration.install().await.map_err(|e| e.to_string())?;
    tracing::info!(integration = id, "Integration installed");
    Ok(())
}

#[tauri::command]
pub async fn toggle_integration(
    id: String,
    enabled: bool,
    state: tauri::State<'_, crate::AppState>,
) -> Result<(), String> {
    // Clone the integration Arc and release the registry lock before start/stop.
    let integration = {
        let reg = state.registry.lock().map_err(|e| e.to_string())?;
        reg.get(&id)
            .ok_or_else(|| format!("Integration '{}' not found", id))?
    };

    if enabled {
        // Auto-install integrations that have not been deployed yet (e.g., Diiisco).
        if integration.installed_version().is_none() {
            integration.install().await.map_err(|e| e.to_string())?;
        }
        integration.start().await.map_err(|e| e.to_string())?;
    } else {
        integration.stop().await.map_err(|e| e.to_string())?;
    }

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

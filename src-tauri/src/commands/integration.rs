use crate::integrations::{HealthStatus, IntegrationStatus, LifecycleState};

#[tauri::command]
pub async fn get_integrations(
    state: tauri::State<'_, crate::AppState>,
) -> Result<Vec<IntegrationStatus>, String> {
    // Snapshot registry metadata without holding the lock across any await point.
    let (entries, total) = {
        let reg = state.registry.lock().map_err(|e| e.to_string())?;
        let total = reg.total_count();
        let entries: Vec<(String, String, bool, Option<String>, bool)> = reg
            .list()
            .iter()
            .map(|i| {
                (
                    i.id().to_string(),
                    i.display_name().to_string(),
                    reg.is_enabled(i.id()),
                    i.installed_version(),
                    i.requires_docker(),
                )
            })
            .collect();
        (entries, total)
    };

    // Read the most recent health check results written by the health loop in main.rs.
    let last = state.last_health.read().map_err(|e| e.to_string())?;
    let last_errors = state.last_integration_error.read().map_err(|e| e.to_string())?;

    let statuses = entries
        .into_iter()
        .map(|(id, display_name, enabled, version, requires_docker)| {
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

            // Healthy-based so the UI matches what the PoC reporter actually
            // submits (reporter proportion counts Healthy only).
            let healthy = matches!(health, HealthStatus::Healthy);

            IntegrationStatus {
                id: id.clone(),
                display_name,
                enabled,
                health,
                lifecycle,
                version,
                poc_contribution: if enabled && healthy { 1.0 / total as f64 } else { 0.0 },
                requires_docker,
                error: last_errors.get(&id).and_then(|e| e.clone()),
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
    // Mutual exclusion: storj ↔ space_acres
    if enabled {
        let (mutually_exclusive_id, other_id) = match id.as_str() {
            "space_acres" => ("storj", "Storj"),
            "storj" => ("space_acres", "SpaceAcres"),
            _ => ("", ""),
        };

        if !mutually_exclusive_id.is_empty() {
            let is_other_enabled = {
                let reg = state.registry.lock().map_err(|e| e.to_string())?;
                reg.is_enabled(mutually_exclusive_id)
            };

            if is_other_enabled {
                return Err(format!(
                    "{} and {} are mutually exclusive. Disable {} first or choose a different integration.",
                    id, other_id, other_id
                ));
            }
        }

        // Check SpaceAcres eligibility
        if id == "space_acres" {
            let (eligible, reason) = crate::integrations::space_acres::SpaceAcresIntegration::check_eligibility().await;
            if !eligible {
                return Err(format!(
                    "{}. Try Storj instead.",
                    reason.unwrap_or_else(|| "SpaceAcres is not eligible on this device".to_string())
                ));
            }
        }

        // Check Mysterium LAN conflicts
        if id == "mysterium" {
            let cfg = state.config.get();
            if !cfg.myst_lan_override {
                if let Some(conflict) = crate::integrations::mysterium_lan_check::scan_lan_conflict()
                    .await
                    .map_err(|e| e.to_string())?
                {
                    return Err(format!(
                        "{}. Enable myst_lan_override in settings to proceed.",
                        conflict
                    ));
                }
            }
        }
    }

    // Clone the integration Arc and release the registry lock before start/stop.
    let integration = {
        let reg = state.registry.lock().map_err(|e| e.to_string())?;
        reg.get(&id)
            .ok_or_else(|| format!("Integration '{}' not found", id))?
    };

    if enabled {
        // Auto-install integrations that have not been deployed yet (e.g., Diiisco).
        if integration.installed_version().is_none() {
            if let Err(e) = integration.install().await {
                let err_msg = e.to_string();
                if let Ok(mut errs) = state.last_integration_error.write() {
                    errs.insert(id.clone(), Some(err_msg.clone()));
                }
                return Err(err_msg);
            }
        }
        if let Err(e) = integration.start().await {
            let err_msg = e.to_string();
            if let Ok(mut errs) = state.last_integration_error.write() {
                errs.insert(id.clone(), Some(err_msg.clone()));
            }
            return Err(err_msg);
        }
        // Clear error on success
        if let Ok(mut errs) = state.last_integration_error.write() {
            errs.insert(id.clone(), None);
        }
    } else {
        if let Err(e) = integration.stop().await {
            let err_msg = e.to_string();
            if let Ok(mut errs) = state.last_integration_error.write() {
                errs.insert(id.clone(), Some(err_msg.clone()));
            }
            return Err(err_msg);
        }
        // Clear error on success
        if let Ok(mut errs) = state.last_integration_error.write() {
            errs.insert(id.clone(), None);
        }
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

use serde::Serialize;
use tauri_plugin_updater::UpdaterExt;

/// Serializable update entry returned to the frontend.
#[derive(Debug, Clone, Serialize)]
pub struct UpdateInfo {
    pub id: String,
    pub name: String,
    pub current_version: Option<String>,
    pub latest_version: Option<String>,
    pub available: bool,
    pub error: Option<String>,
    pub kind: String,
    pub download_url: Option<String>,
    pub body: Option<String>,
}

/// Check for available updates for FEM itself and every registered integration.
/// Errors are isolated per source so a single failure never crashes the whole check.
#[tauri::command]
pub async fn check_updates(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::AppState>,
) -> Result<Vec<UpdateInfo>, String> {
    let mut updates = Vec::new();

    // FEM self-update via the Tauri updater plugin.
    match app
        .updater_builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
    {
        Ok(updater) => match updater.check().await {
            Ok(Some(update)) => {
                let current = env!("CARGO_PKG_VERSION").to_string();
                let available = update.version != current;
                updates.push(UpdateInfo {
                    id: "fem".to_string(),
                    name: "Fry Edge Miner".to_string(),
                    current_version: Some(current),
                    latest_version: Some(update.version.clone()),
                    available,
                    error: None,
                    kind: "app".to_string(),
                    download_url: None, // `Update` exposes body/version; URL is resolved internally by the updater.
                    body: update.body.clone(),
                });
            }
            Ok(None) => {
                updates.push(UpdateInfo {
                    id: "fem".to_string(),
                    name: "Fry Edge Miner".to_string(),
                    current_version: Some(env!("CARGO_PKG_VERSION").to_string()),
                    latest_version: None,
                    available: false,
                    error: None,
                    kind: "app".to_string(),
                    download_url: None,
                    body: None,
                });
            }
            Err(e) => {
                tracing::warn!(error = %e, "FEM self-update check failed");
                updates.push(UpdateInfo {
                    id: "fem".to_string(),
                    name: "Fry Edge Miner".to_string(),
                    current_version: Some(env!("CARGO_PKG_VERSION").to_string()),
                    latest_version: None,
                    available: false,
                    error: Some(e.to_string()),
                    kind: "app".to_string(),
                    download_url: None,
                    body: None,
                });
            }
        },
        Err(e) => {
            tracing::warn!(error = %e, "FEM updater initialization failed");
            updates.push(UpdateInfo {
                id: "fem".to_string(),
                name: "Fry Edge Miner".to_string(),
                current_version: Some(env!("CARGO_PKG_VERSION").to_string()),
                latest_version: None,
                available: false,
                error: Some(e.to_string()),
                kind: "app".to_string(),
                download_url: None,
                body: None,
            });
        }
    }

    // Integration updates. Clone the integration list and release the registry lock
    // before calling out to partner-specific update logic.
    let integrations = {
        let reg = state.registry.lock().map_err(|e| e.to_string())?;
        reg.list()
    };

    for integration in integrations {
        let id = integration.id().to_string();
        let name = integration.display_name().to_string();
        let current_version = integration.installed_version();

        // Always list the integration, even when it has not been installed yet.
        let Some(current_version) = current_version else {
            updates.push(UpdateInfo {
                id,
                name,
                current_version: None,
                latest_version: None,
                available: false,
                error: None,
                kind: "integration".to_string(),
                download_url: None,
                body: None,
            });
            continue;
        };

        match integration.check_update().await {
            Ok(Some(latest)) => {
                let available = current_version != latest;
                updates.push(UpdateInfo {
                    id,
                    name,
                    current_version: Some(current_version),
                    latest_version: Some(latest),
                    available,
                    error: None,
                    kind: "integration".to_string(),
                    download_url: None,
                    body: None,
                });
            }
            Ok(None) => {
                updates.push(UpdateInfo {
                    id,
                    name,
                    current_version: Some(current_version),
                    latest_version: None,
                    available: false,
                    error: None,
                    kind: "integration".to_string(),
                    download_url: None,
                    body: None,
                });
            }
            Err(e) => {
                tracing::warn!(integration = %id, error = %e, "Integration update check failed");
                updates.push(UpdateInfo {
                    id,
                    name,
                    current_version: Some(current_version),
                    latest_version: None,
                    available: false,
                    error: Some(e.to_string()),
                    kind: "integration".to_string(),
                    download_url: None,
                    body: None,
                });
            }
        }
    }

    Ok(updates)
}

/// Install an update for FEM (`kind == "app"`) or an integration (`kind == "integration"`).
/// Returns a human-readable result string ("restart required" for app updates).
#[tauri::command]
pub async fn install_update(
    kind: String,
    id: String,
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::AppState>,
) -> Result<String, String> {
    if kind == "app" {
        if id != "fem" {
            return Err(format!("Unknown app id '{}'; only 'fem' is supported", id));
        }
        let updater = app
            .updater_builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .map_err(|e| e.to_string())?;
        let update = updater
            .check()
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "No update available for Fry Edge Miner".to_string())?;

        update
            .download_and_install(|_chunk, _total| {}, || {})
            .await
            .map_err(|e| e.to_string())?;

        tracing::info!(version = %update.version, "FEM update downloaded and installed");
        Ok("restart required".to_string())
    } else if kind == "integration" {
        tokio::task::block_in_place(|| {
            let reg = state.registry.lock().map_err(|e| e.to_string())?;
            let rt = tokio::runtime::Handle::current();

            let integration = reg
                .get(&id)
                .ok_or_else(|| format!("Integration '{}' not found", id))?;

            let latest = rt
                .block_on(integration.check_update())
                .map_err(|e| e.to_string())?
                .ok_or_else(|| format!("No update available for integration '{}'", id))?;

            rt.block_on(integration.apply_update(&latest))
                .map_err(|e| e.to_string())?;

            tracing::info!(integration = %id, version = %latest, "Integration update applied");
            Ok(format!("Integration '{}' updated to {}", id, latest))
        })
    } else {
        Err(format!("Unknown update kind '{}'", kind))
    }
}

use serde::Serialize;

use crate::migration;

#[derive(Debug, Clone, Serialize)]
pub struct MigrationInfo {
    pub found_keys: Vec<migration::DetectedMinerKey>,
    pub wallet: Option<String>,
    pub suggested_integrations: Vec<String>,
}

#[tauri::command]
pub async fn check_migration() -> Result<Option<MigrationInfo>, String> {
    let installation = migration::detect_fryhub();

    match installation {
        Some(inst) => {
            let plan = migration::plan_migration(&inst, None);
            Ok(Some(MigrationInfo {
                found_keys: inst.found_keys,
                wallet: inst.wallet,
                suggested_integrations: plan.integrations,
            }))
        }
        None => Ok(None),
    }
}

#[tauri::command]
pub async fn run_migration(
    wallet: String,
    state: tauri::State<'_, crate::AppState>,
) -> Result<migration::MigrationResult, String> {
    let installation = migration::detect_fryhub()
        .ok_or("No FryHub installation detected")?;

    let plan = migration::plan_migration(&installation, Some(wallet.clone()));

    // Update config with new FEM key and wallet
    state
        .config
        .update(|cfg| {
            cfg.miner_key = Some(plan.fem_key.clone());
            cfg.wallet_address = Some(wallet);
            for integration_id in &plan.integrations {
                cfg.integrations_enabled
                    .insert(integration_id.clone(), true);
            }
        })
        .map_err(|e| e.to_string())?;

    // Enable mapped integrations in registry
    {
        let mut registry = state.registry.lock().map_err(|e| e.to_string())?;
        for integration_id in &plan.integrations {
            registry.set_enabled(integration_id, true);
        }
    }

    let result = migration::execute_migration(&plan);

    tracing::info!(
        fem_key = result.fem_key,
        integrations = ?result.enabled_integrations,
        "Migration completed"
    );

    Ok(result)
}

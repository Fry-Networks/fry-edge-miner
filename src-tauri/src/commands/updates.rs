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

        // BUG 1/2 (was B7's plain `release_install_tree`): same full
        // pre-install sequence as the background auto-updater — MSI check,
        // partner release + restart-suspend, pre-update binary backup,
        // persisted from/to state — so a manual install from the Updates
        // page gets the same safety net as the automatic path.
        use tauri::Manager;
        let app_data_dir = app
            .path()
            .app_data_dir()
            .map_err(|e| e.to_string())?;
        let exe_path =
            std::env::current_exe().map_err(|e| format!("Could not resolve own exe path: {e}"))?;
        let frynode_path = exe_path
            .parent()
            .map(|d| d.join("resources").join("frynode.exe"));
        let current = env!("CARGO_PKG_VERSION");
        let guard = match crate::updater_auto::prepare_for_update_install(
            &state.supervisor,
            &state.config,
            &app_data_dir,
            &exe_path,
            frynode_path.as_deref(),
            current,
            &update.version,
        )
        .await
        {
            crate::updater_auto::PrepareOutcome::MsiBlocked(entry) => {
                return Err(format!(
                    "An MSI install of Fry Edge Miner is registered ({}). Uninstall it from Apps & Features, then update.",
                    entry.display_name
                ));
            }
            crate::updater_auto::PrepareOutcome::Ready(guard) => guard,
        };

        // C1 review fix: `guard` stays in scope across this call. On
        // failure, `?` returns early and `guard` drops here, resetting
        // UPDATE_IN_PROGRESS — a failed manual install must not permanently
        // disable every integration's restart capability. On success, it is
        // explicitly kept alive below (the app is about to restart).
        update
            .download_and_install(|_chunk, _total| {}, || {})
            .await
            .map_err(|e| e.to_string())?;
        guard.keep_suspended_forever();

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

/// B7 (manual path): the Updates-page install rewrites the same install tree
/// the auto-updater does, so it must release that tree first.
#[cfg(test)]
mod manual_install_tests {
    /// Regression tripwire: the Tauri command itself cannot run under
    /// `cargo test`, but deleting the release call must still fail a test.
    ///
    /// The original version of this test asserted only that the literal string
    /// `release_install_tree` appeared somewhere in the app branch. After
    /// v0.4.28 the branch calls `prepare_for_update_install` instead, and the
    /// only remaining occurrences of that literal in this file were the
    /// assertion itself and a comment — so it passed on prose. Proven vacuous:
    /// with the real `release_install_tree(supervisor).await;` call deleted
    /// from `updater_auto.rs`, the old assertion still passed.
    ///
    /// It now pins the whole chain that actually exists:
    ///   updates.rs app branch -> prepare_for_update_install -> release_install_tree
    /// and strips comments first, so prose can never satisfy it again.
    #[test]
    fn the_manual_app_install_releases_the_install_tree_first() {
        fn code_only(src: &str) -> String {
            src.lines()
                .map(|l| l.split("//").next().unwrap_or(""))
                .collect::<Vec<_>>()
                .join("
")
        }

        // Link 1: the manual app branch goes through the pre-install choke point.
        let app_branch = code_only(
            include_str!("updates.rs")
                .split("else if kind ==")
                .next()
                .expect("updates.rs always has the integration branch"),
        );
        assert!(
            app_branch.contains("prepare_for_update_install"),
            "install_update's app branch no longer goes through the pre-install choke point"
        );

        // Link 2: that choke point still releases the install tree. This is the
        // assertion the old test believed it was making.
        let updater = code_only(include_str!("../updater_auto.rs"));
        let fn_start = updater
            .find("async fn prepare_for_update_install")
            .expect("prepare_for_update_install must exist");
        let body = &updater[fn_start..];
        let body_end = body.find("
pub ").unwrap_or(body.len());
        assert!(
            body[..body_end].contains("release_install_tree("),
            "prepare_for_update_install no longer releases the install tree, so the manual              install can replace a tree partner processes still hold open"
        );
    }
}

/// BUG 10: a genuinely-asserting replacement for the tripwire in
/// `manual_install_tests`.
///
/// That test asserts the literal string `release_install_tree` appears in the
/// app branch — but after v0.4.28 the only two occurrences in this file are the
/// assertion itself and a COMMENT. It passes on the strength of prose and would
/// keep passing if the real call were deleted. The original is deliberately
/// left untouched; this module sits alongside it and strips comments first, so
/// it cannot be satisfied the same way.
#[cfg(test)]
mod bug10_manual_install_gate_tests {
    /// Remove line comments, so no assertion here can be satisfied by prose.
    fn code_only(src: &str) -> String {
        src.lines()
            .map(|l| l.split("//").next().unwrap_or(""))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn app_branch() -> String {
        let src = include_str!("updates.rs");
        let branch = src
            .split("else if kind ==")
            .next()
            .expect("updates.rs always has the integration branch");
        code_only(branch)
    }

    #[test]
    fn the_manual_app_install_honours_every_pre_install_gate_before_downloading() {
        let code = app_branch();
        let prepare = code
            .find("prepare_for_update_install")
            .expect("the manual path must go through the single pre-install choke point");
        let download = code
            .find("download_and_install")
            .expect("the manual path must still install");
        assert!(
            prepare < download,
            "preparation must run BEFORE download_and_install, not after"
        );
    }

    /// The fail-closed MSI outcome did not exist pre-fix, so this is RED on the
    /// old code without needing a mutation argument.
    #[test]
    fn the_manual_app_install_honours_an_inconclusive_msi_probe() {
        let code = app_branch();
        assert!(
            code.contains("MsiBlocked"),
            "must refuse to install over a detected MSI install"
        );
    }

    /// Guards the exact defect in the original tripwire: prove that stripping
    /// comments actually changes what is visible, so this test can never
    /// degrade into the prose-matching one it replaces.
    #[test]
    fn a_comment_alone_cannot_satisfy_these_assertions() {
        let sample = "let x = 1; // prepare_for_update_install download_and_install\n";
        let stripped = code_only(sample);
        assert!(!stripped.contains("prepare_for_update_install"));
        assert!(!stripped.contains("download_and_install"));
    }
}

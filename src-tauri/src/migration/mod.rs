use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tracing::info;

use crate::config::miner_key;

#[cfg(windows)]
const FRYHUB_DATA_DIR: &str = "C:/ProgramData/FryNetworks";

const MINER_PREFIXES: &[(&str, &str)] = &[
    ("BM", "Bandwidth Miner"),
    ("RDN", "Compute Node"),
    ("SDN", "Storage Decentralization Node"),
    ("SVN", "Storage Validator Node"),
    ("IDM", "Indoor Decibel Miner"),
    ("ODM", "Outdoor Decibel Miner"),
    ("ISM", "Indoor Satellite Miner"),
    ("OSM", "Outdoor Satellite Miner"),
];

/// Map from old FryHub miner type to FEM integration IDs
fn map_type_to_integrations(miner_type: &str) -> Vec<&'static str> {
    match miner_type {
        "BM" => vec!["mysterium"],
        "RDN" => vec!["presearch", "diiisco"],
        "SDN" | "SVN" => vec!["space_acres"],
        _ => vec![],
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedMinerKey {
    pub key: String,
    pub miner_type: String,
    pub display_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FryHubInstallation {
    pub found_keys: Vec<DetectedMinerKey>,
    pub wallet: Option<String>,
    pub data_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationPlan {
    pub fem_key: String,
    pub integrations: Vec<String>,
    pub wallet: Option<String>,
    pub source_keys: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationResult {
    pub fem_key: String,
    pub enabled_integrations: Vec<String>,
    pub migrated_keys: Vec<String>,
}

/// Detect FryHub installation by scanning a given data directory
pub fn detect_fryhub_at(data_dir_path: &str) -> Option<FryHubInstallation> {
    let data_dir = PathBuf::from(data_dir_path);
    if !data_dir.exists() {
        info!("No FryHub data directory found at {}", data_dir_path);
        return None;
    }

    let mut found_keys = Vec::new();

    // Scan for miner-{CODE} subdirectories
    if let Ok(entries) = std::fs::read_dir(&data_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("miner-") {
                let code = name.strip_prefix("miner-").unwrap_or("");
                // Look for miner key files inside the directory
                if let Some(key) = find_miner_key_in_dir(&entry.path(), code) {
                    found_keys.push(key);
                }
            }
        }
    }

    if found_keys.is_empty() {
        info!("FryHub directory exists but no active miner keys found");
        return None;
    }

    info!(
        count = found_keys.len(),
        "Detected FryHub installation with active miners"
    );

    Some(FryHubInstallation {
        found_keys,
        wallet: None, // Wallet detection from FryHub config is complex — user provides during migration
        data_dir: data_dir_path.to_string(),
    })
}

/// Detect FryHub installation at the default data directory
#[cfg(windows)]
pub fn detect_fryhub() -> Option<FryHubInstallation> {
    detect_fryhub_at(FRYHUB_DATA_DIR)
}

#[cfg(not(windows))]
pub fn detect_fryhub() -> Option<FryHubInstallation> {
    None // FryHub legacy data only existed on Windows
}

/// Search a miner directory for key files
fn find_miner_key_in_dir(dir: &Path, code: &str) -> Option<DetectedMinerKey> {
    // Check for config/miner_key.txt or similar key storage
    let key_file = dir.join("config").join("miner_key.txt");
    if key_file.exists() {
        if let Ok(content) = std::fs::read_to_string(&key_file) {
            let key = content.trim().to_string();
            if !key.is_empty() {
                let display = MINER_PREFIXES
                    .iter()
                    .find(|(p, _)| *p == code)
                    .map(|(_, n)| n.to_string())
                    .unwrap_or_else(|| format!("Unknown ({})", code));
                return Some(DetectedMinerKey {
                    key,
                    miner_type: code.to_string(),
                    display_name: display,
                });
            }
        }
    }

    // Also check directory name pattern for active installs
    if dir.join("config").exists() {
        let display = MINER_PREFIXES
            .iter()
            .find(|(p, _)| *p == code)
            .map(|(_, n)| n.to_string())
            .unwrap_or_else(|| format!("Unknown ({})", code));
        return Some(DetectedMinerKey {
            key: format!("{}-DETECTED", code),
            miner_type: code.to_string(),
            display_name: display,
        });
    }

    None
}

/// Plan a migration from FryHub miner keys to FEM
pub fn plan_migration(
    installation: &FryHubInstallation,
    wallet: Option<String>,
) -> MigrationPlan {
    let fem_key = miner_key::generate();

    let mut integrations: Vec<String> = Vec::new();
    let source_keys: Vec<String> = installation
        .found_keys
        .iter()
        .map(|k| k.key.clone())
        .collect();

    for key in &installation.found_keys {
        for integration in map_type_to_integrations(&key.miner_type) {
            if !integrations.contains(&integration.to_string()) {
                integrations.push(integration.to_string());
            }
        }
    }

    info!(
        fem_key = fem_key,
        integrations = ?integrations,
        source_count = source_keys.len(),
        "Migration plan created"
    );

    MigrationPlan {
        fem_key,
        integrations,
        wallet: wallet.or(installation.wallet.clone()),
        source_keys,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detection_and_mapping() {
        let fixture = std::env::var("FEM_TEST_FIXTURE_DIR")
            .expect("FEM_TEST_FIXTURE_DIR must be set to fixture path");

        // FAIL CLOSED: detection MUST return Some
        let inst = detect_fryhub_at(&fixture)
            .expect("detect_fryhub_at must find installations in fixture dir");

        // FAIL CLOSED: must find expected keys
        assert!(
            !inst.found_keys.is_empty(),
            "Must detect at least one miner key"
        );

        let plan = plan_migration(&inst, Some("TESTADDR".to_string()));

        // BM fallback detection (config dir exists, no miner_key.txt)
        let bm_found = inst.found_keys.iter().any(|k| k.miner_type == "BM");
        assert!(bm_found, "BM miner must be detected (fallback path)");

        // RDN maps to presearch+diiisco
        let rdn_found = inst.found_keys.iter().any(|k| k.miner_type == "RDN");
        assert!(rdn_found, "RDN miner must be detected");
        assert!(
            plan.integrations.contains(&"presearch".to_string()),
            "RDN must map to presearch"
        );
        assert!(
            plan.integrations.contains(&"diiisco".to_string()),
            "RDN must map to diiisco"
        );

        // SDN maps to space_acres
        let sdn_found = inst.found_keys.iter().any(|k| k.miner_type == "SDN");
        assert!(sdn_found, "SDN miner must be detected");
        assert!(
            plan.integrations.contains(&"space_acres".to_string()),
            "SDN must map to space_acres"
        );

        // Verify RDN has the fake test key (not production)
        let rdn_key = inst
            .found_keys
            .iter()
            .find(|k| k.miner_type == "RDN")
            .unwrap();
        assert!(
            rdn_key.key.contains("TESTFIXTURE"),
            "RDN key must be test fixture key, not production"
        );

        // No production API calls: this test only calls detect + plan,
        // never execute_migration or notify_migration
    }
}

/// Execute the migration plan
pub fn execute_migration(plan: &MigrationPlan) -> MigrationResult {
    info!(fem_key = plan.fem_key, "Executing migration");

    MigrationResult {
        fem_key: plan.fem_key.clone(),
        enabled_integrations: plan.integrations.clone(),
        migrated_keys: plan.source_keys.clone(),
    }
}

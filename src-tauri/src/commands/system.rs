use serde::Serialize;

use crate::integrations::docker_manager::{docker_status, status_user_message, DockerStatus};

#[derive(Debug, Clone, Serialize)]
pub struct SystemStatus {
    pub docker: DockerStatus,
    pub docker_message: String,
    pub virtualization_supported: bool,
}

/// System prerequisite snapshot for the frontend (Docker state drives the
/// availability display of Presearch/Diiisco cards).
#[tauri::command]
pub async fn get_system_status() -> Result<SystemStatus, String> {
    // docker info + the CIM virtualization probe are blocking subprocesses.
    tokio::task::spawn_blocking(|| {
        let docker = docker_status();
        SystemStatus {
            docker,
            docker_message: status_user_message(docker),
            virtualization_supported:
                crate::integrations::docker_manager::virtualization_supported(),
        }
    })
    .await
    .map_err(|e| e.to_string())
}

pub mod health;
pub mod platform;
pub mod process;

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::integrations::HealthStatus;
use process::ManagedProcess;

/// Serializable process info for IPC
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessInfo {
    pub integration_id: String,
    pub pid: u32,
    pub running: bool,
}

/// Manages all child processes for integrations
pub struct Supervisor {
    processes: HashMap<String, ManagedProcess>,
    log_dir: PathBuf,
}

impl Supervisor {
    pub fn new(log_dir: PathBuf) -> Self {
        Self {
            processes: HashMap::new(),
            log_dir,
        }
    }

    /// Start an integration process
    pub fn start_integration(
        &mut self,
        id: &str,
        command: &str,
        args: &[&str],
    ) -> std::io::Result<()> {
        if let Some(existing) = self.processes.get_mut(id) {
            if existing.is_running() {
                info!(integration = id, "Already running, skipping start");
                return Ok(());
            }
        }
        let integration_log_dir = self.log_dir.join(id);
        let process = ManagedProcess::spawn(id, command, args, &integration_log_dir)?;
        info!(
            integration = id,
            pid = process.pid(),
            "Integration started"
        );
        self.processes.insert(id.to_string(), process);
        Ok(())
    }

    /// Stop an integration process gracefully
    pub fn stop_integration(&mut self, id: &str) -> std::io::Result<()> {
        if let Some(mut process) = self.processes.remove(id) {
            process.stop(Duration::from_secs(10))?;
            info!(integration = id, "Integration stopped");
        }
        Ok(())
    }

    /// Stop all integration processes
    pub fn stop_all(&mut self) {
        let ids: Vec<String> = self.processes.keys().cloned().collect();
        for id in ids {
            if let Err(e) = self.stop_integration(&id) {
                warn!(integration = id, error = %e, "Failed to stop integration");
            }
        }
    }

    /// Get the health status of an integration process
    pub fn get_status(&mut self, id: &str) -> HealthStatus {
        match self.processes.get_mut(id) {
            Some(process) => {
                if process.is_running() {
                    HealthStatus::Healthy
                } else {
                    HealthStatus::Stopped
                }
            }
            None => HealthStatus::Stopped,
        }
    }

    /// Get info for all managed processes
    pub fn list_processes(&mut self) -> Vec<ProcessInfo> {
        self.processes
            .iter_mut()
            .map(|(id, proc)| ProcessInfo {
                integration_id: id.clone(),
                pid: proc.pid(),
                running: proc.is_running(),
            })
            .collect()
    }

    /// Clean shutdown of all processes
    pub fn shutdown(&mut self) {
        info!("Supervisor shutting down all integrations");
        self.stop_all();
    }
}

impl Drop for Supervisor {
    fn drop(&mut self) {
        self.shutdown();
    }
}

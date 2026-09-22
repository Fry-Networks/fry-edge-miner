pub mod health;
/// B10: the restart-pause resume — that it is bounded, what the card says
/// about it, and that it actually re-arms. Its own file so health.rs's three
/// existing test modules stay byte-identical.
#[cfg(test)]
mod health_rearm_tests;
pub mod platform;
pub mod process;
pub mod resource_guard;

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
        self.start_integration_with_env(id, command, args, &[])
    }

    /// Start an integration with extra environment (BUG 6: secrets belong in
    /// the environment, never on the command line).
    pub fn start_integration_with_env(
        &mut self,
        id: &str,
        command: &str,
        args: &[&str],
        env: &[(&str, &str)],
    ) -> std::io::Result<()> {
        if let Some(existing) = self.processes.get_mut(id) {
            if existing.is_running() {
                info!(integration = id, "Already running, skipping start");
                return Ok(());
            }
        }
        let integration_log_dir = self.log_dir.join(id);
        // BUG 9: run every managed partner in its own always-writable directory
        // instead of letting it inherit FEM's CWD (`C:\Windows\System32` when
        // FEM starts from its Run key), which is what made frynode fail with
        // `mkdir node-identity: Access is denied`.
        let working_dir =
            process::working_dir_for(id, &crate::integrations::download::partners_base_dir());
        let process = ManagedProcess::spawn_full(
            id,
            command,
            args,
            &integration_log_dir,
            Some(working_dir.as_path()),
            env,
        )?;
        info!(integration = id, pid = process.pid(), "Integration started");
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

    /// Restart a crashed integration by stopping (if tracked) and re-spawning.
    pub fn restart_integration(
        &mut self,
        id: &str,
        command: &str,
        args: &[&str],
    ) -> std::io::Result<()> {
        let _ = self.stop_integration(id);
        self.start_integration(id, command, args)
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

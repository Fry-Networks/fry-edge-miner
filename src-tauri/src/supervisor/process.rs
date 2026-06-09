use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use chrono::{DateTime, Utc};
use tracing::{info, warn};

/// A managed child process with metadata
pub struct ManagedProcess {
    child: Child,
    pub integration_id: String,
    pub started_at: DateTime<Utc>,
    pub log_dir: PathBuf,
}

impl ManagedProcess {
    /// Spawn a new child process with stdout/stderr redirected to log files
    pub fn spawn(
        integration_id: &str,
        command: &str,
        args: &[&str],
        log_dir: &Path,
    ) -> io::Result<Self> {
        std::fs::create_dir_all(log_dir)?;
        let stdout_path = log_dir.join(format!("{}_stdout.log", integration_id));
        let stderr_path = log_dir.join(format!("{}_stderr.log", integration_id));
        let stdout_file = std::fs::File::create(&stdout_path)?;
        let stderr_file = std::fs::File::create(&stderr_path)?;

        info!(
            integration = integration_id,
            command = command,
            "Spawning process"
        );

        let child = Command::new(command)
            .args(args)
            .stdout(Stdio::from(stdout_file))
            .stderr(Stdio::from(stderr_file))
            .spawn()?;

        Ok(Self {
            child,
            integration_id: integration_id.to_string(),
            started_at: Utc::now(),
            log_dir: log_dir.to_path_buf(),
        })
    }

    /// Check if the process is still running (non-blocking)
    pub fn is_running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Get the process ID
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Stop the process. Sends kill signal, waits up to timeout.
    pub fn stop(&mut self, timeout: Duration) -> io::Result<()> {
        info!(
            integration = self.integration_id,
            pid = self.child.id(),
            "Stopping process"
        );

        self.child.kill()?;

        let start = std::time::Instant::now();
        loop {
            match self.child.try_wait()? {
                Some(_status) => return Ok(()),
                None if start.elapsed() >= timeout => {
                    warn!(
                        integration = self.integration_id,
                        "Process did not exit within timeout"
                    );
                    return Ok(());
                }
                None => std::thread::sleep(Duration::from_millis(100)),
            }
        }
    }
}

impl Drop for ManagedProcess {
    fn drop(&mut self) {
        if self.is_running() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

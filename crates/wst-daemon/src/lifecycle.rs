//! Daemon lifecycle management

use crate::DaemonState;
use anyhow::Result;
use std::sync::Arc;

/// Check if another daemon instance is already running
pub fn check_singleton() -> Result<bool> {
    // TODO: Implement singleton detection via IPC ping
    // For now, always return false
    Ok(false)
}

/// Ensure only one daemon instance is running
pub fn ensure_singleton() -> Result<()> {
    if check_singleton()? {
        Err(anyhow::anyhow!("Another WST daemon instance is already running"))
    } else {
        Ok(())
    }
}

/// Install daemon as Windows service (future)
#[cfg(windows)]
pub fn install_service() -> Result<()> {
    // TODO: Implement Windows service installation
    Err(anyhow::anyhow!("Service installation not yet implemented"))
}

/// Uninstall daemon service
#[cfg(windows)]
pub fn uninstall_service() -> Result<()> {
    // TODO: Implement Windows service uninstallation
    Err(anyhow::anyhow!("Service uninstallation not yet implemented"))
}

/// Run daemon as service (future)
pub async fn run_as_service() -> Result<()> {
    // TODO: Implement Windows service runner
    Err(anyhow::anyhow!("Service mode not yet implemented"))
}

/// Daemon lifecycle manager
pub struct LifecycleManager {
    state: Arc<DaemonState>,
}

impl LifecycleManager {
    /// Create a new lifecycle manager
    pub fn new(state: Arc<DaemonState>) -> Self {
        Self { state }
    }

    /// Initialize the daemon
    pub async fn initialize(&self) -> Result<()> {
        tracing::info!("Initializing WST daemon");

        // Ensure singleton
        ensure_singleton()?;

        // Restore sessions
        let ids = self.state.session_manager.restore_sessions().await?;
        tracing::info!("Restored {} sessions", ids.len());

        Ok(())
    }

    /// Shutdown the daemon gracefully
    pub async fn shutdown(&self) -> Result<()> {
        tracing::info!("Shutting down WST daemon");

        // Signal shutdown
        self.state.shutdown().await;

        // Snapshot sessions
        self.state.session_manager.snapshot_all().await?;

        tracing::info!("WST daemon shutdown complete");
        Ok(())
    }

    /// Check if daemon should restart
    pub fn should_restart(&self) -> bool {
        // TODO: Implement restart logic
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_singleton_check() {
        // This test will pass if no daemon is running
        let result = check_singleton();
        assert!(result.is_ok());
    }
}

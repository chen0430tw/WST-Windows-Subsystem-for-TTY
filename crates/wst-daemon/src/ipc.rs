//! IPC (Inter-Process Communication) between daemon and frontend

use crate::DaemonState;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// IPC message types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IpcMessage {
    /// Ping to check if daemon is alive
    Ping,
    /// Pong response
    Pong,
    /// Request to show frontend
    ShowFrontend,
    /// Request to hide frontend
    HideFrontend,
    /// Request to toggle frontend visibility
    ToggleFrontend,
    /// Request daemon shutdown
    Shutdown,
    /// Query session list
    ListSessions,
    /// Response with session list
    SessionList(Vec<SessionInfo>),
    /// Request to create a new session
    CreateSession { name: String, backend: String },
    /// Response with session ID
    SessionCreated(u64),
    /// Request to switch to a session
    SwitchSession(u64),
    /// Request to close a session
    CloseSession(u64),
    /// Execute a command in a session
    Execute { session_id: u64, command: String },
    /// Command output
    Output { session_id: u64, text: String, is_error: bool },
    /// Error message
    Error(String),
}

/// Information about a session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub id: u64,
    pub name: String,
    pub backend: String,
    pub state: String,
    pub task_count: usize,
    pub persistent: bool,
}

/// IPC server - simplified version using file-based communication
pub async fn run_ipc_server(state: Arc<DaemonState>) -> Result<()> {
    tracing::info!("IPC server starting (file-based mode)");

    // For now, just keep the task alive
    // TODO: Implement actual IPC communication
    while !state.is_shutting_down().await {
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    }

    Ok(())
}

/// IPC client for communicating with the daemon
pub struct IpcClient {
    _daemon_path: String,
}

impl IpcClient {
    /// Create a new IPC client
    pub fn new() -> Self {
        Self {
            _daemon_path: "wst-daemon".to_string(),
        }
    }

    /// Check if daemon is running (via IPC file/socket)
    pub async fn ping(&self) -> bool {
        // TODO: Implement actual ping via IPC
        false
    }

    /// Request to show frontend
    pub async fn show_frontend(&self) -> Result<()> {
        // TODO: Implement via IPC
        Ok(())
    }

    /// Request to toggle frontend
    pub async fn toggle_frontend(&self) -> Result<()> {
        // TODO: Implement via IPC
        Ok(())
    }

    /// Get list of sessions
    pub async fn list_sessions(&self) -> Result<Vec<SessionInfo>> {
        // TODO: Implement via IPC
        Ok(Vec::new())
    }

    /// Create a new session
    pub async fn create_session(&self, name: String, backend: String) -> Result<u64> {
        // TODO: Implement via IPC
        Ok(0)
    }

    /// Switch to a session
    pub async fn switch_session(&self, id: u64) -> Result<()> {
        // TODO: Implement via IPC
        Ok(())
    }

    /// Close a session
    pub async fn close_session(&self, id: u64) -> Result<()> {
        // TODO: Implement via IPC
        Ok(())
    }

    /// Request daemon shutdown
    pub async fn shutdown(&self) -> Result<()> {
        // TODO: Implement via IPC
        Ok(())
    }
}

impl Default for IpcClient {
    fn default() -> Self {
        Self::new()
    }
}

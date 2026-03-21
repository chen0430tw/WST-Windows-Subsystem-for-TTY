//! IPC (Inter-Process Communication) between daemon and frontend

use crate::DaemonState;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[cfg(windows)]
use tokio::net::windows::named_pipe::NamedPipeServer;

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

/// Named pipe name for WST IPC
const PIPE_NAME: &str = r"\\.\pipe\WST_DAEMON_IPC";

/// IPC server using Windows named pipes
#[cfg(windows)]
pub async fn run_ipc_server(state: Arc<DaemonState>) -> Result<()> {
    use std::time::Duration;
    use tokio::net::windows::named_pipe::ServerOptions;

    tracing::info!("IPC server starting on {}", PIPE_NAME);

    // Track if we've logged the first error (to avoid spam)
    let mut logged_error = false;

    loop {
        if state.is_shutting_down().await {
            break;
        }

        // Create named pipe server
        let server = ServerOptions::new()
            .first_pipe_instance(true)
            .create(PIPE_NAME);

        match server {
            Ok(pipe) => {
                logged_error = false; // Reset on success
                tracing::debug!("Named pipe created, waiting for client...");

                let state_clone = state.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_ipc_client(pipe, state_clone).await {
                        tracing::debug!("IPC client handler error: {}", e);
                    }
                });

                // Wait before trying to create another pipe instance
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            Err(_e) => {
                // Only log once to avoid spam
                if !logged_error {
                    tracing::warn!("IPC pipe busy (first instance exists), will retry silently...");
                    logged_error = true;
                }
                // For first_pipe_instance, subsequent attempts will fail with ERROR_ACCESS_DENIED
                // This is expected - just wait and retry
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        }
    }

    tracing::info!("IPC server stopped");
    Ok(())
}

/// IPC server stub for non-Windows platforms
#[cfg(not(windows))]
pub async fn run_ipc_server(state: Arc<DaemonState>) -> Result<()> {
    tracing::info!("IPC server starting (stub mode - not Windows)");

    while !state.is_shutting_down().await {
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    }

    Ok(())
}

/// Handle an IPC client connection
#[cfg(windows)]
async fn handle_ipc_client(mut pipe: NamedPipeServer, state: Arc<DaemonState>) -> Result<()> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    pipe.connect().await?;
    tracing::debug!("IPC client connected");

    let mut line = String::new();

    loop {
        line.clear();

        // Read request
        let mut reader = BufReader::new(&mut pipe);
        let bytes_read = reader.read_line(&mut line).await?;
        drop(reader); // Release the borrow

        if bytes_read == 0 {
            break; // Client disconnected
        }

        tracing::debug!("IPC received: {}", line.trim());

        // Parse message
        let response = match serde_json::from_str::<IpcMessage>(&line) {
            Ok(msg) => handle_ipc_message(msg, &state).await?,
            Err(e) => {
                tracing::error!("Failed to parse IPC message: {}", e);
                IpcMessage::Error(format!("Invalid message: {}", e))
            }
        };

        // Send response
        let response_json = serde_json::to_string(&response)?;
        pipe.write_all(format!("{}\n", response_json).as_bytes()).await?;
        pipe.flush().await?;
    }

    Ok(())
}

/// Handle an IPC message and return response
async fn handle_ipc_message(msg: IpcMessage, state: &DaemonState) -> Result<IpcMessage> {
    match msg {
        IpcMessage::Ping => Ok(IpcMessage::Pong),
        IpcMessage::ShowFrontend => {
            state.set_frontend_visible(true).await;
            Ok(IpcMessage::Pong)
        }
        IpcMessage::HideFrontend => {
            state.set_frontend_visible(false).await;
            Ok(IpcMessage::Pong)
        }
        IpcMessage::ToggleFrontend => {
            state.toggle_frontend().await;
            Ok(IpcMessage::Pong)
        }
        IpcMessage::Shutdown => {
            state.shutdown().await;
            Ok(IpcMessage::Pong)
        }
        IpcMessage::ListSessions => {
            let sessions = state.session_manager.list_sessions().await;
            let session_infos = sessions
                .into_iter()
                .map(|s| SessionInfo {
                    id: s.id(),
                    name: s.name().to_string(),
                    backend: format!("{:?}", s.backend),
                    state: format!("{:?}", s.state()),
                    task_count: s.task_count(),
                    persistent: s.persistent,
                })
                .collect();
            Ok(IpcMessage::SessionList(session_infos))
        }
        IpcMessage::CreateSession { name, backend } => {
            use wst_protocol::BackendKind;
            use wst_session::SessionConfig;

            let backend_kind = match backend.to_lowercase().as_str() {
                "cygctl" => BackendKind::Cygctl,
                "pwsh" | "powershell" => BackendKind::Pwsh,
                "cmd" => BackendKind::Cmd,
                _ => return Ok(IpcMessage::Error(format!("Unknown backend: {}", backend))),
            };

            let config = SessionConfig {
                name,
                backend: backend_kind,
                ..Default::default()
            };

            match state.session_manager.create_session(config).await {
                Ok(id) => Ok(IpcMessage::SessionCreated(id)),
                Err(e) => Ok(IpcMessage::Error(format!("Failed to create session: {}", e))),
            }
        }
        IpcMessage::SwitchSession(id) => {
            match state.session_manager.switch_session(id).await {
                Ok(()) => Ok(IpcMessage::Pong),
                Err(e) => Ok(IpcMessage::Error(format!("Failed to switch session: {}", e))),
            }
        }
        IpcMessage::CloseSession(id) => {
            match state.session_manager.close_session(id).await {
                Ok(()) => Ok(IpcMessage::Pong),
                Err(e) => Ok(IpcMessage::Error(format!("Failed to close session: {}", e))),
            }
        }
        IpcMessage::Execute { session_id, command } => {
            // This is handled by the backend, not directly by daemon
            // For now, just acknowledge
            tracing::info!("Execute request for session {}: {}", session_id, command);
            Ok(IpcMessage::Pong)
        }
        _ => Ok(IpcMessage::Error("Unknown message".to_string())),
    }
}

/// IPC client for communicating with the daemon
pub struct IpcClient {
    pipe_path: String,
}

impl IpcClient {
    /// Create a new IPC client
    pub fn new() -> Self {
        Self {
            pipe_path: PIPE_NAME.to_string(),
        }
    }

    /// Check if daemon is running (via IPC file/socket)
    pub async fn ping(&self) -> bool {
        self.send_message(IpcMessage::Ping).await.is_ok()
    }

    /// Request to show frontend
    pub async fn show_frontend(&self) -> Result<()> {
        self.send_message(IpcMessage::ShowFrontend).await?;
        Ok(())
    }

    /// Request to toggle frontend
    pub async fn toggle_frontend(&self) -> Result<()> {
        self.send_message(IpcMessage::ToggleFrontend).await?;
        Ok(())
    }

    /// Get list of sessions
    pub async fn list_sessions(&self) -> Result<Vec<SessionInfo>> {
        let response = self.send_message(IpcMessage::ListSessions).await?;
        match response {
            IpcMessage::SessionList(sessions) => Ok(sessions),
            IpcMessage::Error(e) => Err(anyhow::anyhow!("{}", e)),
            _ => Err(anyhow::anyhow!("Unexpected response")),
        }
    }

    /// Create a new session
    pub async fn create_session(&self, name: String, backend: String) -> Result<u64> {
        let response = self.send_message(IpcMessage::CreateSession { name, backend }).await?;
        match response {
            IpcMessage::SessionCreated(id) => Ok(id),
            IpcMessage::Error(e) => Err(anyhow::anyhow!("{}", e)),
            _ => Err(anyhow::anyhow!("Unexpected response")),
        }
    }

    /// Switch to a session
    pub async fn switch_session(&self, id: u64) -> Result<()> {
        self.send_message(IpcMessage::SwitchSession(id)).await?;
        Ok(())
    }

    /// Close a session
    pub async fn close_session(&self, id: u64) -> Result<()> {
        self.send_message(IpcMessage::CloseSession(id)).await?;
        Ok(())
    }

    /// Request daemon shutdown
    pub async fn shutdown(&self) -> Result<()> {
        self.send_message(IpcMessage::Shutdown).await?;
        Ok(())
    }

    /// Send a message and wait for response
    async fn send_message(&self, msg: IpcMessage) -> Result<IpcMessage> {
        #[cfg(windows)]
        {
            use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
            use tokio::net::windows::named_pipe::ClientOptions;

            // Try to connect - open() is synchronous in tokio's Windows named pipe
            let client_result = ClientOptions::new().open(&self.pipe_path);

            let mut client = match client_result {
                Ok(c) => c,
                Err(e) => return Err(anyhow::anyhow!("Failed to connect to daemon: {}", e)),
            };

            // Send request
            let request_json = serde_json::to_string(&msg)?;
            client.write_all(format!("{}\n", request_json).as_bytes()).await?;
            client.flush().await?;

            // Read response
            let mut reader = BufReader::new(&mut client);
            let mut line = String::new();
            let bytes_read = reader.read_line(&mut line).await?;

            if bytes_read == 0 {
                return Err(anyhow::anyhow!("No response from daemon"));
            }

            serde_json::from_str::<IpcMessage>(&line.trim())
                .map_err(|e| anyhow::anyhow!("Failed to parse response: {}", e))
        }

        #[cfg(not(windows))]
        {
            // Stub for non-Windows
            let _ = msg;
            Err(anyhow::anyhow!("IPC not supported on this platform"))
        }
    }
}

impl Default for IpcClient {
    fn default() -> Self {
        Self::new()
    }
}

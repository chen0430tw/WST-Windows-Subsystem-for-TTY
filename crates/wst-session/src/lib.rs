//! # WST Session Management
//!
//! This crate provides multi-session support for WST.
//!
//! Each session is an independent execution context with its own:
//! - Backend instance (cygctl/Pwsh/Cmd)
//! - Environment variables
//! - Working directory
//! - Command history
//! - Running tasks

pub mod manager;
pub mod session;
pub mod store;

pub use manager::{SessionManager, SessionManagerConfig};
pub use session::{Session, SessionConfig, SessionSnapshot, SessionState};
pub use store::SessionStore;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum SessionError {
    #[error("Session not found: {0}")]
    SessionNotFound(u64),

    #[error("Session already exists: {0}")]
    SessionAlreadyExists(u64),

    #[error("Session is locked: {0}")]
    SessionLocked(u64),

    #[error("Backend error: {0}")]
    BackendError(String),

    #[error("Store error: {0}")]
    StoreError(String),

    #[error("Invalid session name: {0}")]
    InvalidName(String),
}

pub type Result<T> = std::result::Result<T, SessionError>;

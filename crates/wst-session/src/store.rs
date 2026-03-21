use crate::{Result, SessionError};
use crate::session::SessionSnapshot;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use tokio::sync::RwLock;

/// Store for persisting session snapshots
pub struct SessionStore {
    /// Directory where session snapshots are stored
    store_dir: PathBuf,
    /// In-memory cache of snapshots
    cache: RwLock<HashMap<u64, SessionSnapshot>>,
}

impl SessionStore {
    /// Create a new session store
    pub fn new<P: AsRef<Path>>(store_dir: P) -> Result<Self> {
        let store_dir = store_dir.as_ref().to_path_buf();

        // Create store directory if it doesn't exist
        fs::create_dir_all(&store_dir)
            .map_err(|e| SessionError::StoreError(format!("Failed to create store dir: {}", e)))?;

        Ok(Self {
            store_dir,
            cache: RwLock::new(HashMap::new()),
        })
    }

    /// Get the default store directory
    pub fn default_dir() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".wst")
            .join("sessions")
    }

    /// Create store with default directory
    pub fn with_default_dir() -> Result<Self> {
        Self::new(Self::default_dir())
    }

    /// Save a session snapshot
    pub async fn save(&self, snapshot: SessionSnapshot) -> Result<()> {
        let id = snapshot.id;

        // Update cache
        self.cache.write().await.insert(id, snapshot.clone());

        // Write to file
        let path = self.session_path(id);
        let json = serde_json::to_string_pretty(&snapshot)
            .map_err(|e| SessionError::StoreError(format!("Failed to serialize: {}", e)))?;

        fs::write(&path, json)
            .map_err(|e| SessionError::StoreError(format!("Failed to write: {}", e)))?;

        tracing::debug!("Saved session {} to {:?}", id, path);
        Ok(())
    }

    /// Load a session snapshot
    pub async fn load(&self, id: u64) -> Result<SessionSnapshot> {
        // Check cache first
        if let Some(snapshot) = self.cache.read().await.get(&id) {
            return Ok(snapshot.clone());
        }

        // Load from file
        let path = self.session_path(id);
        let json = fs::read_to_string(&path)
            .map_err(|_| SessionError::SessionNotFound(id))?;

        let snapshot: SessionSnapshot = serde_json::from_str(&json)
            .map_err(|e| SessionError::StoreError(format!("Failed to deserialize: {}", e)))?;

        // Update cache
        self.cache.write().await.insert(id, snapshot.clone());

        Ok(snapshot)
    }

    /// List all stored session IDs
    pub async fn list(&self) -> Result<Vec<u64>> {
        let mut ids = Vec::new();

        let entries = fs::read_dir(&self.store_dir)
            .map_err(|e| SessionError::StoreError(format!("Failed to read store: {}", e)))?;

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                let name = path.file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("");
                if let Ok(id) = name.parse::<u64>() {
                    ids.push(id);
                }
            }
        }

        ids.sort();
        Ok(ids)
    }

    /// Delete a session snapshot
    pub async fn delete(&self, id: u64) -> Result<()> {
        // Remove from cache
        self.cache.write().await.remove(&id);

        // Delete file
        let path = self.session_path(id);
        fs::remove_file(&path)
            .map_err(|e| SessionError::StoreError(format!("Failed to delete: {}", e)))?;

        tracing::debug!("Deleted session {}", id);
        Ok(())
    }

    /// Delete all session snapshots
    pub async fn clear(&self) -> Result<()> {
        // Clear cache
        self.cache.write().await.clear();

        // Delete all files
        let entries = fs::read_dir(&self.store_dir)
            .map_err(|e| SessionError::StoreError(format!("Failed to read store: {}", e)))?;

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                fs::remove_file(&path)
                    .map_err(|e| SessionError::StoreError(format!("Failed to delete: {}", e)))?;
            }
        }

        tracing::debug!("Cleared all sessions");
        Ok(())
    }

    /// Get the path to a session file
    fn session_path(&self, id: u64) -> PathBuf {
        self.store_dir.join(format!("{}.json", id))
    }

    /// Restore all sessions from the store
    pub async fn restore_all(&self) -> Result<Vec<SessionSnapshot>> {
        let ids = self.list().await?;
        let mut snapshots = Vec::new();

        for id in ids {
            match self.load(id).await {
                Ok(snapshot) => snapshots.push(snapshot),
                Err(e) => {
                    tracing::warn!("Failed to load session {}: {}", id, e);
                }
            }
        }

        Ok(snapshots)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wst_protocol::BackendKind;
    use chrono::Utc;

    #[tokio::test]
    async fn test_store_save_load() {
        let temp_dir = std::env::temp_dir().join("wst_test_store");
        let _ = fs::remove_dir_all(&temp_dir);

        let store = SessionStore::new(&temp_dir).unwrap();

        let snapshot = SessionSnapshot {
            id: 123,
            name: "test".to_string(),
            backend: BackendKind::Cygctl,
            state: crate::SessionState::Ready,
            created_at: Utc::now(),
            cwd: "C:\\".to_string(),
            env: HashMap::new(),
            history: vec!["ls".to_string()],
        };

        store.save(snapshot.clone()).await.unwrap();

        let loaded = store.load(123).await.unwrap();
        assert_eq!(loaded.id, 123);
        assert_eq!(loaded.name, "test");

        let ids = store.list().await.unwrap();
        assert_eq!(ids, vec![123]);

        store.delete(123).await.unwrap();
        let ids = store.list().await.unwrap();
        assert_eq!(ids.len(), 0);

        let _ = fs::remove_dir_all(&temp_dir);
    }
}

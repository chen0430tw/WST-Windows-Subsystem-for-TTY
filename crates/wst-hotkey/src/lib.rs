use anyhow::Result;

pub struct HotkeyManager;

impl HotkeyManager {
    pub fn new() -> Self {
        Self
    }

    pub fn register_global_hotkey(&self, _hotkey: &str) -> Result<()> {
        Ok(())
    }
}

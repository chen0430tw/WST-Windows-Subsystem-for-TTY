//! Global hotkey handling for WST daemon

use crate::DaemonState;
use anyhow::Result;
use std::sync::Arc;
use tokio::time::Duration;

// Hotkey modifier constants
const MOD_ALT: u32 = 0x0001;
const MOD_CONTROL: u32 = 0x0002;
const MOD_SHIFT: u32 = 0x0004;
const MOD_WIN: u32 = 0x0008;

/// Hotkey configuration
#[derive(Debug, Clone)]
pub struct HotkeyConfig {
    /// Virtual key code
    pub vk: u32,
    /// Modifiers (CTRL, ALT, SHIFT)
    pub modifiers: u32,
}

impl HotkeyConfig {
    /// Create default hotkey (Ctrl+Alt+Space)
    pub fn default_wst_hotkey() -> Self {
        Self {
            vk: 0x20, // VK_SPACE
            modifiers: MOD_CONTROL | MOD_ALT,
        }
    }

    /// Parse from string (e.g., "Ctrl+Alt+Space")
    pub fn parse(s: &str) -> Result<Self> {
        let parts: Vec<&str> = s.split('+').collect();
        let mut modifiers = 0u32;
        let mut vk = None;

        for part in parts {
            match part.trim().to_uppercase().as_str() {
                "CTRL" | "CONTROL" => modifiers |= MOD_CONTROL,
                "ALT" => modifiers |= MOD_ALT,
                "SHIFT" => modifiers |= MOD_SHIFT,
                "WIN" | "WINDOWS" => modifiers |= MOD_WIN,
                "SPACE" => vk = Some(0x20), // VK_SPACE
                "F1" => vk = Some(0x70), // VK_F1
                "F2" => vk = Some(0x71),
                "F3" => vk = Some(0x72),
                "F4" => vk = Some(0x73),
                "F5" => vk = Some(0x74),
                "F6" => vk = Some(0x75),
                "F7" => vk = Some(0x76),
                "F8" => vk = Some(0x77),
                "F9" => vk = Some(0x78),
                "F10" => vk = Some(0x79),
                "F11" => vk = Some(0x7A),
                "F12" => vk = Some(0x7B),
                _ => {
                    // Try to parse as single character
                    if part.len() == 1 {
                        let c = part.chars().next().unwrap() as u8;
                        if c.is_ascii_alphabetic() {
                            vk = Some(c.to_ascii_uppercase() as u32);
                        }
                    }
                }
            }
        }

        let vk = vk.ok_or_else(|| anyhow::anyhow!("No virtual key found in hotkey string"))?;

        Ok(Self { vk, modifiers })
    }
}

/// Run the hotkey listener
pub async fn run_hotkey_listener(state: Arc<DaemonState>) -> Result<()> {
    #[cfg(windows)]
    {
        tracing::info!("Hotkey listener starting (Ctrl+Alt+Space)");

        // TODO: Implement actual Windows hotkey registration
        // For now, just keep the task alive
        while !state.is_shutting_down().await {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }

        tracing::info!("Hotkey listener stopped");
    }

    #[cfg(not(windows))]
    {
        tracing::warn!("Hotkey support is only available on Windows");
        while !state.is_shutting_down().await {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hotkey_parse() {
        let config = HotkeyConfig::parse("Ctrl+Alt+Space").unwrap();
        assert_eq!(config.vk, 0x20); // VK_SPACE
        assert_eq!(config.modifiers, MOD_CONTROL | MOD_ALT);
    }

    #[test]
    fn test_default_hotkey() {
        let config = HotkeyConfig::default_wst_hotkey();
        assert_eq!(config.vk, 0x20); // VK_SPACE
        assert_eq!(config.modifiers, MOD_CONTROL | MOD_ALT);
    }
}

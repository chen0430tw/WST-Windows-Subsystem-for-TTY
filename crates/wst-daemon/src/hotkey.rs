//! Global hotkey handling for WST daemon

use crate::DaemonState;
use anyhow::Result;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::Duration;

#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::*;

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
    /// Create default hotkey (Ctrl+Alt+F12) - F12 is less likely to conflict
    pub fn default_wst_hotkey() -> Self {
        Self {
            vk: 0x7B, // VK_F12
            modifiers: MOD_CONTROL | MOD_ALT,
        }
    }

    /// Parse from string (e.g., "Ctrl+Alt+F3")
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
                "F3" => vk = Some(0x72), // VK_F3 - default WST hotkey
                "F4" => vk = Some(0x73),
                "F5" => vk = Some(0x74),
                "F6" => vk = Some(0x75),
                "F7" => vk = Some(0x76),
                "F8" => vk = Some(0x77),
                "F9" => vk = Some(0x78),
                "F10" => vk = Some(0x79),
                "F11" => vk = Some(0x7A),
                "F12" => vk = Some(0x7B), // VK_F12 - default WST hotkey
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

    /// Get the combined modifiers and vk
    pub fn as_modifiers_and_vk(&self) -> (u32, u32) {
        (self.modifiers, self.vk)
    }
}

/// Hotkey event sent to the daemon
#[derive(Debug, Clone)]
pub enum HotkeyEvent {
    /// Toggle frontend visibility
    ToggleFrontend,
    /// Show frontend
    ShowFrontend,
    /// Hide frontend
    HideFrontend,
    /// Custom hotkey with ID
    Custom(u32),
}

/// Run the hotkey listener
pub async fn run_hotkey_listener(
    state: Arc<DaemonState>,
    mut event_rx: mpsc::Receiver<HotkeyEvent>,
) -> Result<()> {
    tracing::info!("Hotkey listener starting");

    // Track if UI is currently running
    let mut ui_process: Option<std::process::Child> = None;

    while !state.is_shutting_down().await {
        tokio::select! {
            Some(event) = event_rx.recv() => {
                match event {
                    HotkeyEvent::ToggleFrontend => {
                        let visible = state.toggle_frontend().await;
                        tracing::info!("Hotkey: Frontend toggled (now visible: {})", visible);

                        if visible {
                            // Show/launch UI
                            launch_or_focus_ui(&mut ui_process).await?;
                        } else {
                            // Hide UI (close it)
                            close_ui(&mut ui_process).await?;
                        }
                    }
                    HotkeyEvent::ShowFrontend => {
                        state.set_frontend_visible(true).await;
                        tracing::info!("Hotkey: Frontend shown");
                        launch_or_focus_ui(&mut ui_process).await?;
                    }
                    HotkeyEvent::HideFrontend => {
                        state.set_frontend_visible(false).await;
                        tracing::info!("Hotkey: Frontend hidden");
                        close_ui(&mut ui_process).await?;
                    }
                    HotkeyEvent::Custom(id) => {
                        tracing::debug!("Hotkey: Custom event {}", id);
                    }
                }
            }
            _ = tokio::time::sleep(Duration::from_secs(1)) => {
                // Continue
            }
        }
    }

    // Clean up UI process on exit
    if let Some(mut child) = ui_process {
        let _ = child.kill();
    }

    tracing::info!("Hotkey listener stopped");
    Ok(())
}

/// Launch or focus the WST UI
async fn launch_or_focus_ui(ui_process: &mut Option<std::process::Child>) -> Result<()> {
    use std::process::Command;

    tracing::info!("=== launch_or_focus_ui() called ===");

    // Check if UI is already running
    if let Some(child) = ui_process {
        tracing::info!("Checking existing UI process with PID: {:?}", child.id());
        match child.try_wait() {
            Ok(None) => {
                // Process still running - show the window
                tracing::info!("UI process still running (None = still alive), calling show_ui_window()");
                show_ui_window()?;
                return Ok(());
            }
            Ok(Some(status)) => {
                tracing::info!("UI process exited with status: {:?}", status);
            }
            Err(e) => {
                tracing::warn!("Failed to wait for UI process: {}", e);
            }
        }
    } else {
        tracing::info!("No existing UI process tracked");
    }

    // Check if wst-ui is already running (maybe started separately)
    tracing::info!("Checking if wst-ui.exe is running externally");
    if is_ui_process_running() {
        tracing::info!("WST UI is already running (external), calling show_ui_window()");
        show_ui_window()?;
        return Ok(());
    } else {
        tracing::info!("wst-ui.exe not found running externally");
    }

    // Launch UI from the correct directory (project root)
    // This ensures it can find config files
    let exe_path = find_wst_ui_executable();
    tracing::info!("UI executable path: {:?}", exe_path);

    // Get the absolute path and set working directory to project root
    let exe_abs = std::path::Path::new(&exe_path)
        .canonicalize()
        .map_err(|e| anyhow::anyhow!("Failed to resolve UI path: {}", e))?;

    // Set working directory to the project root (where wst.toml would be)
    let project_root = exe_abs.parent()
        .and_then(|p| p.parent())
        .unwrap_or_else(|| std::path::Path::new("."));

    tracing::info!("Working directory: {:?}", project_root);

    let child = Command::new(&exe_path)
        .current_dir(project_root)
        .spawn()
        .map_err(|e| anyhow::anyhow!("Failed to launch UI: {}", e))?;

    tracing::info!("WST UI launched with PID: {:?}", child.id());

    *ui_process = Some(child);
    Ok(())
}

/// Close the WST UI
async fn close_ui(ui_process: &mut Option<std::process::Child>) -> Result<()> {
    tracing::info!("=== close_ui() called ===");

    if let Some(child) = ui_process {
        tracing::info!("UI process exists with PID: {:?}", child.id());

        // Try to hide the window instead of killing the process
        #[cfg(windows)]
        {
            tracing::info!("Attempting to hide WST UI window (keeping process alive)");
            match hide_ui_window() {
                Ok(()) => {
                    tracing::info!("UI window hidden successfully");
                    return Ok(());
                }
                Err(e) => {
                    tracing::warn!("Hide window failed: {}, falling back to kill process", e);
                }
            }
        }

        // Fallback: kill the process
        tracing::info!("Closing UI (PID: {:?})", child.id());
        let _ = child.kill();
        let _ = child.wait();
        *ui_process = None;
    }
    Ok(())
}

/// Show the WST UI window using Windows API
#[cfg(windows)]
fn show_ui_window() -> Result<()> {
    use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SetForegroundWindow, SW_RESTORE, GetWindowTextW};
    use windows::Win32::Foundation::{HWND, LPARAM, BOOL};
    use winput::{Vk, Action, Input};
    use std::thread;
    use std::time::Duration;
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;

    tracing::info!("=== show_ui_window() called ===");

    unsafe {
        // Use static mut to pass data to callback (unsafe but works for this case)
        static mut FOUND_HWND: Option<HWND> = None;
        static mut WINDOW_COUNT: u32 = 0;

        unsafe extern "system" fn enum_callback(hwnd: HWND, _lparam: LPARAM) -> BOOL {
            WINDOW_COUNT += 1;

            // Get window title
            let mut buffer = [u16::default(); 512];
            let len = GetWindowTextW(hwnd, &mut buffer);

            if len > 0 {
                let title_string = OsString::from_wide(&buffer[..len as usize])
                    .to_string_lossy()
                    .to_string();

                // Only log non-IME windows to reduce noise
                if !title_string.contains("IME") && !title_string.contains("MSCTF") {
                    tracing::info!("Window #{}: '{}'", WINDOW_COUNT, title_string);
                }

                // Check if title contains "WST" - be more flexible
                if title_string.contains("WST") {
                    tracing::info!("Found WST window: '{}' with hwnd: {:?}", title_string, hwnd);
                    FOUND_HWND = Some(hwnd);
                    return BOOL::from(false); // FALSE = Stop enumeration
                }
            }
            BOOL::from(true) // TRUE = Continue enumeration
        }

        FOUND_HWND = None;
        WINDOW_COUNT = 0;
        let enum_result = EnumWindows(Some(enum_callback), LPARAM(0));
        tracing::info!("EnumWindows result: {:?}, Total windows checked: {}", enum_result, WINDOW_COUNT);

        if let Some(hwnd) = FOUND_HWND {
            tracing::info!("Window is valid, proceeding to show");

            // Show and bring to front
            tracing::info!("Calling ShowWindow with SW_RESTORE");
            let result = ShowWindow(hwnd, SW_RESTORE);
            tracing::info!("ShowWindow result: {:?}", result);

            tracing::info!("Calling SetForegroundWindow");
            let fg_result = SetForegroundWindow(hwnd);
            tracing::info!("SetForegroundWindow result: {:?}", fg_result);

            thread::sleep(Duration::from_millis(100));

            // Send F11 to toggle Windows Terminal fullscreen
            tracing::info!("Sending F11 keystroke for Windows Terminal fullscreen");
            let f11_result = winput::send(Vk::F11);
            tracing::info!("F11 send result: {:?}", f11_result);
            thread::sleep(Duration::from_millis(200));

            // Send Alt+Enter to toggle legacy console fullscreen
            tracing::info!("Sending Alt+Enter for legacy console fullscreen");
            let inputs = vec![
                Input::from_vk(Vk::LeftMenu, Action::Press),
                Input::from_vk(Vk::Enter, Action::Press),
                Input::from_vk(Vk::Enter, Action::Release),
                Input::from_vk(Vk::LeftMenu, Action::Release),
            ];
            tracing::info!("Alt+Enter inputs prepared ({} inputs)", inputs.len());
            let alt_enter_result = winput::send_inputs(&inputs);
            tracing::info!("Alt+Enter send result: {:?}", alt_enter_result);

            tracing::info!("=== show_ui_window() completed successfully ===");
            return Ok(());
        }

        tracing::warn!("Could not find WST UI window");
        Err(anyhow::anyhow!("UI window not found"))
    }
}

/// Hide the WST UI window using Windows API
/// For fullscreen console windows, we need to send F11/Alt+Enter first
#[cfg(windows)]
fn hide_ui_window() -> Result<()> {
    use windows::Win32::UI::WindowsAndMessaging::{SetForegroundWindow, ShowWindow, SW_HIDE, GetWindowTextW, GetWindowThreadProcessId};
    use windows::Win32::Foundation::{HWND, LPARAM, BOOL};
    use winput::{Vk, Action, Input};
    use std::thread;
    use std::time::Duration;
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;

    tracing::info!("=== hide_ui_window() called ===");

    unsafe {
        // Use static mut to pass data to callback (unsafe but works for this case)
        static mut FOUND_HWND: Option<HWND> = None;
        static mut WINDOW_COUNT: u32 = 0;

        unsafe extern "system" fn enum_callback(hwnd: HWND, _lparam: LPARAM) -> BOOL {
            WINDOW_COUNT += 1;

            // Get window title
            let mut buffer = [u16::default(); 512];
            let len = GetWindowTextW(hwnd, &mut buffer);

            if len > 0 {
                let title_string = OsString::from_wide(&buffer[..len as usize])
                    .to_string_lossy()
                    .to_string();

                // Only log non-IME windows to reduce noise
                if !title_string.contains("IME") && !title_string.contains("MSCTF") {
                    tracing::info!("Window #{}: '{}'", WINDOW_COUNT, title_string);
                }

                // Check if title contains "WST" - be more flexible
                if title_string.contains("WST") {
                    tracing::info!("Found WST window: '{}' with hwnd: {:?}", title_string, hwnd);
                    FOUND_HWND = Some(hwnd);
                    return BOOL::from(false); // FALSE = Stop enumeration
                }
            }
            BOOL::from(true) // TRUE = Continue enumeration
        }

        FOUND_HWND = None;
        WINDOW_COUNT = 0;
        let enum_result = EnumWindows(Some(enum_callback), LPARAM(0));
        tracing::info!("EnumWindows result: {:?}, Total windows checked: {}", enum_result, WINDOW_COUNT);

        if let Some(hwnd) = FOUND_HWND {
            tracing::info!("Window is valid, proceeding to hide");

            // Bring window to foreground first
            tracing::info!("Calling SetForegroundWindow");
            let fg_result = SetForegroundWindow(hwnd);
            tracing::info!("SetForegroundWindow result: {:?}", fg_result);
            thread::sleep(Duration::from_millis(100));

            // Send F11 to toggle Windows Terminal fullscreen (exit)
            tracing::info!("Sending F11 keystroke to exit Windows Terminal fullscreen");
            let f11_result = winput::send(Vk::F11);
            tracing::info!("F11 send result: {:?}", f11_result);
            thread::sleep(Duration::from_millis(200));

            // Send Alt+Enter to toggle legacy console fullscreen (exit)
            tracing::info!("Sending Alt+Enter to exit legacy console fullscreen");
            let inputs = vec![
                Input::from_vk(Vk::LeftMenu, Action::Press),
                Input::from_vk(Vk::Enter, Action::Press),
                Input::from_vk(Vk::Enter, Action::Release),
                Input::from_vk(Vk::LeftMenu, Action::Release),
            ];
            tracing::info!("Alt+Enter inputs prepared ({} inputs)", inputs.len());
            let alt_enter_result = winput::send_inputs(&inputs);
            tracing::info!("Alt+Enter send result: {:?}", alt_enter_result);
            thread::sleep(Duration::from_millis(200));

            // Finally hide the window
            tracing::info!("Calling ShowWindow with SW_HIDE");
            let result = ShowWindow(hwnd, SW_HIDE);
            tracing::info!("ShowWindow SW_HIDE result: {:?}", result);

            tracing::info!("=== hide_ui_window() completed successfully ===");
            return Ok(());
        }

        tracing::warn!("Could not find WST UI window");
        Err(anyhow::anyhow!("UI window not found"))
    }
}

/// Find window by title and show/hide it (non-Windows stub)
#[cfg(not(windows))]
fn show_ui_window() -> Result<()> {
    Err(anyhow::anyhow!("Window show/hide not supported on this platform"))
}

#[cfg(not(windows))]
fn hide_ui_window() -> Result<()> {
    Err(anyhow::anyhow!("Window show/hide not supported on this platform"))
}

/// Check if wst-ui process is already running
#[cfg(windows)]
fn is_ui_process_running() -> bool {
    use std::process::Command;

    // Use tasklist to check if wst-ui.exe is running
    let output = Command::new("tasklist")
        .args(["/FI", "IMAGENAME eq wst-ui.exe"])
        .output();

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            // Check if output contains the process name (not just the header)
            stdout.lines().count() > 1
        }
        Err(_) => false,
    }
}

#[cfg(not(windows))]
fn is_ui_process_running() -> bool {
    false
}

/// Find the wst-ui executable
fn find_wst_ui_executable() -> String {
    // Try multiple paths
    let paths = vec![
        "target/release/wst-ui.exe",
        "../target/release/wst-ui.exe",
        "../../target/release/wst-ui.exe",
        "wst-ui.exe",
    ];

    for path in paths {
        if std::path::Path::new(path).exists() {
            return path.to_string();
        }
    }

    // Default path
    "target/release/wst-ui.exe".to_string()
}

/// Start the hotkey manager in a background thread using win-hotkeys
#[cfg(windows)]
pub fn start_hotkey_thread(
    config: HotkeyConfig,
    event_tx: mpsc::Sender<HotkeyEvent>,
) -> Result<std::thread::JoinHandle<()>> {
    use std::thread;
    use win_hotkeys::VKey;

    let (modifiers, vk) = config.as_modifiers_and_vk();

    tracing::info!("=== Starting hotkey thread ===");
    tracing::info!("Config: modifiers={:#x}, vk={:#x}", modifiers, vk);

    // Convert our virtual key code to win-hotkeys VKey
    let trigger_key = vk_to_vkey(vk)?;
    tracing::info!("Trigger key: {:?}", trigger_key);

    // Convert our modifiers to VKey slice
    let mut mod_keys = Vec::new();
    if modifiers & MOD_CONTROL != 0 {
        mod_keys.push(VKey::Control);
        tracing::info!("Added Control modifier");
    }
    if modifiers & MOD_ALT != 0 {
        mod_keys.push(VKey::LMenu); // LMenu = Left Alt
        tracing::info!("Added Alt modifier (LMenu)");
    }
    if modifiers & MOD_SHIFT != 0 {
        mod_keys.push(VKey::Shift);
        tracing::info!("Added Shift modifier");
    }
    if modifiers & MOD_WIN != 0 {
        mod_keys.push(VKey::LWin);
        tracing::info!("Added Win modifier");
    }

    let handle = thread::spawn(move || {
        tracing::info!("Hotkey thread started, creating HotkeyManager");

        // Create hotkey manager
        let mut hm = win_hotkeys::HotkeyManager::new();

        // Register the hotkey
        tracing::info!("Attempting to register hotkey:");
        tracing::info!("  Trigger: {:?}", trigger_key);
        tracing::info!("  Modifiers: {:?}", mod_keys);

        match hm.register_hotkey(trigger_key, &mod_keys, move || {
            tracing::info!("*** HOTKEY TRIGGERED! ***");
            tracing::info!("Sending ToggleFrontend event...");

            match event_tx.try_send(HotkeyEvent::ToggleFrontend) {
                Ok(_) => tracing::info!("Event sent successfully"),
                Err(e) => tracing::error!("Failed to send event: {}", e),
            }
        }) {
            Ok(id) => {
                tracing::info!("Hotkey registered successfully!");
                tracing::info!("Hotkey ID: {:?}", id);
                tracing::info!("Press Ctrl+Alt+F12 to toggle WST UI...");
            }
            Err(e) => {
                tracing::error!("Failed to register hotkey: {}", e);
            }
        }

        // Run the event loop
        tracing::info!("Entering event loop...");
        hm.event_loop();

        tracing::info!("Hotkey event loop exited");
    });

    Ok(handle)
}

/// Convert virtual key code to win-hotkeys VKey
#[cfg(windows)]
fn vk_to_vkey(vk: u32) -> Result<win_hotkeys::VKey> {
    use win_hotkeys::VKey;

    Ok(match vk {
        0x20 => VKey::Space,
        0x30 => VKey::Vk0,
        0x31 => VKey::Vk1,
        0x32 => VKey::Vk2,
        0x33 => VKey::Vk3,
        0x34 => VKey::Vk4,
        0x35 => VKey::Vk5,
        0x36 => VKey::Vk6,
        0x37 => VKey::Vk7,
        0x38 => VKey::Vk8,
        0x39 => VKey::Vk9,
        0x41 => VKey::A,
        0x42 => VKey::B,
        0x43 => VKey::C,
        0x44 => VKey::D,
        0x45 => VKey::E,
        0x46 => VKey::F,
        0x47 => VKey::G,
        0x48 => VKey::H,
        0x49 => VKey::I,
        0x4A => VKey::J,
        0x4B => VKey::K,
        0x4C => VKey::L,
        0x4D => VKey::M,
        0x4E => VKey::N,
        0x4F => VKey::O,
        0x50 => VKey::P,
        0x51 => VKey::Q,
        0x52 => VKey::R,
        0x53 => VKey::S,
        0x54 => VKey::T,
        0x55 => VKey::U,
        0x56 => VKey::V,
        0x57 => VKey::W,
        0x58 => VKey::X,
        0x59 => VKey::Y,
        0x5A => VKey::Z,
        0x70 => VKey::F1,
        0x71 => VKey::F2,
        0x72 => VKey::F3,
        0x73 => VKey::F4,
        0x74 => VKey::F5,
        0x75 => VKey::F6,
        0x76 => VKey::F7,
        0x77 => VKey::F8,
        0x78 => VKey::F9,
        0x79 => VKey::F10,
        0x7A => VKey::F11,
        0x7B => VKey::F12,
        _ => VKey::from_vk_code(vk as u16),
    })
}

/// Start the hotkey manager in a background thread (non-Windows stub)
#[cfg(not(windows))]
pub fn start_hotkey_thread(
    _config: HotkeyConfig,
    _event_tx: mpsc::Sender<HotkeyEvent>,
) -> Result<std::thread::JoinHandle<()>> {
    use std::thread;

    let handle = thread::spawn(move || {
        tracing::warn!("Hotkey support is only available on Windows");
        loop {
            thread::sleep(std::time::Duration::from_secs(1));
        }
    });

    Ok(handle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hotkey_parse() {
        let config = HotkeyConfig::parse("Ctrl+Alt+F12").unwrap();
        assert_eq!(config.vk, 0x7B); // VK_F12
        assert_eq!(config.modifiers, MOD_CONTROL | MOD_ALT);
    }

    #[test]
    fn test_hotkey_parse_f1() {
        let config = HotkeyConfig::parse("Ctrl+F1").unwrap();
        assert_eq!(config.vk, 0x70); // VK_F1
        assert_eq!(config.modifiers, MOD_CONTROL);
    }

    #[test]
    fn test_hotkey_parse_shift_ctrl_a() {
        let config = HotkeyConfig::parse("Shift+Ctrl+A").unwrap();
        assert_eq!(config.vk, b'A' as u32);
        assert_eq!(config.modifiers, MOD_SHIFT | MOD_CONTROL);
    }

    #[test]
    fn test_default_hotkey() {
        let config = HotkeyConfig::default_wst_hotkey();
        assert_eq!(config.vk, 0x7B); // VK_F12
        assert_eq!(config.modifiers, MOD_CONTROL | MOD_ALT);
    }

    #[test]
    fn test_hotkey_modifiers_and_vk() {
        let config = HotkeyConfig::default_wst_hotkey();
        let (modifiers, vk) = config.as_modifiers_and_vk();
        assert_eq!(modifiers, MOD_CONTROL | MOD_ALT);
        assert_eq!(vk, 0x7B); // VK_F12
    }
}

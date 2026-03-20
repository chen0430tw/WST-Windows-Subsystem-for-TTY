# WST

WST (Windows Subsystem for TTY) is a Windows-hosted pseudo-TTY subsystem.

## Current stage

MVP skeleton only.

## Workspace

- `apps/wst-ui`: frontend entry
- `crates/wst-core`: core session logic
- `crates/wst-backend`: backend abstraction
- `crates/wst-protocol`: shared protocol types
- `crates/wst-config`: config loading
- `crates/wst-hotkey`: hotkey manager stub
- `native/wst_native`: native Windows runtime placeholder

## Run

```powershell
cargo run -p wst-ui
```

## Notes

- This is an initial skeleton, not the finished subsystem.
- `Cygctl` backend is currently a placeholder and still routes to the stub backend.
- Native hotkey / ConPTY / platform work is reserved for later integration.

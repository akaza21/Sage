# Sage footprint control

This is a disposable matched Tauri/WebView2 control for ADE-0004. It is not a
Sage product target and is outside the production frontend and Rust inputs.

It intentionally matches:

- Tauri 2.11.5 and tauri-build 2.6.3;
- Sage's custom protocol and release profile;
- window dimensions and supported window settings;
- CSP, `core:default` capability, and WebView2 process/security model;
- the repository's pinned Rust toolchain.

It intentionally omits React, SQLite, Git, editor state, persistence, and all
Sage services so its process tree measures the local Tauri/WebView2 floor.

Build from the repository root:

```powershell
cargo build --manifest-path tools/footprint-control/Cargo.toml --release --features custom-protocol --offline
```

Measure using `scripts/measure-windows-footprint.ps1` with
`-Scenario control -Acceptance`.

# Sage

Sage is a small, persistent, ACP-native Agent Development Environment.

The product is intentionally narrower than a VS Code fork:

- chats, files, per-file diffs, and terminals share one tab strip;
- agent edits appear in their originating chat and open as per-file diff tabs;
- project and chat navigation lives on the left;
- Explorer, Search, Source Control, Problems, and Outline live on the right;
- the Rust host owns persistence, filesystem, Git, PTYs, and ACP processes;
- the WebView is a disposable projection;
- Claude Code, Codex, and Pi are curated, install-on-demand ACP harnesses;
- harness selection always happens before model selection;
- ordinary folders and multi-root projects are the default;
- worktrees are optional.

## Current implementation frontier

ADE-0005 durable project lifecycle and multi-root resource identity:

```text
SQLite project/root registry
  -> canonical ResourceRef { rootId, relativePath }
  -> authoritative serialized project transitions
  -> bounded validated layout and dirty-buffer recovery
  -> per-root Explorer, editor, Diff, and contained Git projections
```

Ordinary folders can create projects through the native picker, projects can
hold up to eight ordered roots, and up to 64 projects restore their own tabs,
selection, sessions, messages, and dirty buffers. The WebView never receives
absolute paths. Late work from an old project or selection is rejected, and a
post-commit projection failure blocks for bounded authoritative recovery rather
than reviving stale UI.

The final custom-protocol executable is 5,118,976 bytes and the unsigned NSIS
installer is 1,848,425 bytes. Five clean packaged launches put Chat at 231.766
MiB median / 233.211 MiB P95 and Explorer at 232.207 MiB median / 234.762 MiB
P95 private bytes on WebView2 150.0.4078.65. CSP, sandboxing, process
isolation, GPU acceleration, accessibility, and the minimal `core:default`
capability remain enabled.

ADE-0005 implementation self-check is complete, but fresh-context independent
review and independent verification have not been recorded. See
`docs/development/CURRENT-STATE.md`; no ADE-0006 work item is specified.

Syntax highlighting, LSP, project search, Git mutation/history, file watching,
PTYs, and real Claude Code, Codex, and Pi ACP processes remain subsequent work.

## Stack

- Tauri 2 and Rust
- React 19, TypeScript, and Vite
- Zustand for bounded frontend view state
- SQLite WAL for durable state
- ACP Rust SDK v1 behind an isolated Cargo feature
- Vitest and Rust unit tests

## Prerequisites

- Node.js 22.12 or newer
- Rust 1.88 or newer with the stable MSVC toolchain
- Microsoft C++ Build Tools with the Windows SDK
- WebView2

On Windows, follow the official Tauri prerequisites before running the desktop
application. The current development machine has these prerequisites installed.

## Commands

```powershell
npm install
npm run dev
npm run check
node scripts/validate-performance-budgets.mjs
```

Native local gates (Windows, macOS, Linux):

```powershell
npm run gate:static   # policy, prettier, eslint, tsc, rustfmt (about a minute)
npm run gate:fast     # static + Vitest + strict Clippy
npm run gate:rust     # three-phase Rust tests (parallel, then git-process and preview serially)
npm run gate:full     # fast + rust + budgets + build + bundle cap
npm run smoke         # headless rendered-app walkthrough with screenshots
npm run lane new X    # one-writer worktree for ticket X; `lane done X` integrates to main
```

After the native prerequisites are installed:

```powershell
npm run tauri dev
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings
```

## Project boundaries

Read:

- `docs/architecture/README.md`
- `docs/development/WORKFLOW.md`
- `docs/development/CURRENT-STATE.md`
- `docs/decisions/ADR-0001-process-and-state-ownership.md`
- `docs/decisions/ADR-0002-chat-native-diffs-and-navigation.md`
- `docs/decisions/ADR-0003-bounded-native-repository-services.md`
- `docs/decisions/ADR-0004-durable-conflict-safe-editor-buffers.md`
- `docs/decisions/ADR-0005-windows-footprint-attribution.md`
- `docs/decisions/ADR-0006-durable-project-and-resource-identity.md`
- `docs/work-items/ADE-0001-walking-skeleton/brief.md`
- `docs/work-items/ADE-0002-repository-substrate/brief.md`
- `docs/work-items/ADE-0003-persistent-editor-buffers/brief.md`
- `docs/work-items/ADE-0003-persistent-editor-buffers/results.md`
- `docs/work-items/ADE-0004-windows-idle-footprint/brief.md`
- `docs/work-items/ADE-0004-windows-idle-footprint/results.md`
- `docs/work-items/ADE-0005-project-lifecycle/brief.md`
- `docs/work-items/ADE-0005-project-lifecycle/results.md`

The base installer must not grow a bundled Node runtime, Chromium, Monaco,
libgit2, a VS Code extension host, or built-in harness runtimes.

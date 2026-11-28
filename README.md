# Sage

Sage is a small, persistent, ACP-native Agent Development Environment.

The product is intentionally narrower than a VS Code fork:

- chats, files, per-file diffs, and terminals share one tab strip;
- agent edits appear in their originating chat and open as per-file diff tabs;
- project and chat navigation lives on the left;
- Explorer, Search, Source Control, Problems, and Outline live on the right;
- the Rust host owns persistence, filesystem, Git, PTYs, and ACP processes;
- the WebView is a disposable projection;
- Codex CLI, Claude Code, and Pi are first-class Sage ADE targets under
  ADR-0008, with separately staged managed-release admission;
- harness selection always happens before model selection;
- ordinary folders and multi-root projects are the default;
- worktrees are optional.

## Current implementation frontier

Durable project lifecycle and multi-root resource identity:

```text
SQLite project/root registry
  -> canonical ResourceRef { rootId, relativePath }
  -> authoritative serialized project transitions
  -> bounded validated layout and dirty-buffer recovery
  -> per-root Explorer, editor, Diff, and contained Git projections
```

Ordinary folders can create projects through the native picker, projects can
hold up to eight ordered roots, and up to 64 projects restore their own tabs,
selection, sessions, messages, and dirty buffers. Native project/resource paths
remain native-only. Harness catalog projections likewise never send absolute
executable paths to the WebView: executable identity stays in Rust, and the
catalog sends curated IDs, availability metadata, and at most a 32-character
dotted numeric display version (not raw CLI output). This is a native identity
projection guarantee, not a promise to scrub user-authored file/chat content.
Boot-time unsupervised CLI version execution remains a known, separately
deferred defect; this path repair does not change observed-local Codex admission.
Late work from an old project or selection is rejected, and a post-commit
projection failure blocks for bounded authoritative recovery rather than
reviving stale UI.

CSP, sandboxing, process isolation, GPU acceleration, accessibility, and the
minimal `core:default` capability remain enabled.

Syntax highlighting, LSP, project search, Git mutation/history, file watching,
PTYs, and managed-release wiring for Claude Code, Codex CLI, and Pi remain
staged work. Observed-local or dormant harness source is not release admission.

## Windows ACP convergence guarantee

For a launched Windows ACP tree, `ZeroProcessToken` means the retained root
process is signaled, the owned Job reports no active processes, and the
supervisor's stderr drain has completed. The Job remains owned with
no-breakaway and kill-on-close armed until final `TreeControl` drop; issuing a
token does not itself close the Job. This is not proof that all descendant
process objects have signaled or that every file handle has been released.
Callers must not use the token alone as permission to delete a directory or
assume filesystem locks are gone. No upper bound on descendant object
settlement was established, and the Cargo-target lease contract defect is
still outstanding.

## Stack

- Tauri 2 and Rust
- React 19, TypeScript, and Vite
- Zustand for bounded frontend view state
- SQLite WAL for durable state
- ACP Rust SDK v1 behind an isolated Cargo feature
- Vitest and Rust unit tests

## Install on macOS

1. Open a successful [Desktop packages workflow run](https://github.com/akaza21/Sage/actions/workflows/desktop-packages.yml) on `main`.
2. Download the `Sage-macOS-universal-<commit>` artifact under **Artifacts** and unzip it. It supports Apple Silicon and Intel Macs on macOS 11 or later.
3. Open the DMG, drag **Sage** into **Applications**, then eject the disk image.
4. Open Sage from **Applications**. This personal build uses an ad hoc signature. If macOS blocks it and you trust the build, open **System Settings → Privacy & Security** and use **Open Anyway** for Sage. The artifact includes its commit and SHA-256 checksum.

Sage runs the Claude Code CLI installed on your Mac. Use the native installer from the [Claude Code setup guide](https://code.claude.com/docs/en/setup), then run `claude auth login` in Terminal if sign-in is needed. Reopen Sage and select **Claude Code** in the Agent composer. The SDK binding requires a supported 2.x version from 2.1.300 onward.

To build a fresh installer on GitHub, select **Run workflow → main** in **Desktop packages**. The run also produces a Windows installer for the same commit.

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
npm run dev         # real UI in a browser tab against the real debug backend
npm run check
```

`npm run dev` (`scripts/dev-live.mjs`) starts Vite on a free loopback port (`--port <n>` or `SAGE_UI_PORT` pick one; `--jobs <n>` sets `CARGO_BUILD_JOBS`) and `tauri dev --features browser-bridge` against it, both at below-normal priority, and prints one URL. The debug Sage uses a throwaway profile, never your real one. It lives outside the checkout and is private to you: in `~/.cache/sage-dev/<checkout>-<hash>` (on Linux `$XDG_RUNTIME_DIR/sage-dev/...` when set). Not `%LOCALAPPDATA%`: a packaged Windows app, such as an agent host, redirects its processes' writes there into a private copy. On Linux both `sage-dev` folders must be yours, real directories and mode 700, or the launcher refuses to start. So the dev server (whose root is the checkout) can never serve its pairing code or database; the launcher prints the path. The dev server also sends no CORS headers and refuses dot-env, key, `.git` and `.sage-dev` paths. Cargo builds into this checkout's own `target/` unless you set `CARGO_TARGET_DIR`; keep it off any target dir that gates or other sessions share, or a rebuild waits on their build lock. Each backend run publishes its bridge port and single-use pairing code into that profile, and the dev server serves it to same-origin page scripts at `/__sage/bridge-run`, so the tab pairs by itself. TypeScript edits hot-reload; a Rust edit rebuilds the backend, and the tab shows "Reconnecting to the Sage backend…", waits for the new run and reloads itself. Only one `npm run dev` runs per checkout; a second refuses and leaves the first alone. Ctrl+C stops both children and everything they started. Not yet on macOS: WebKit ignores the per-profile data directory, so the debug app could share browser data with your real Sage; `npm run dev` refuses there.

The bridge listens on 127.0.0.1 only, checks the Host and Origin of every connection, and cannot be compiled into release builds; the run route exists only on the dev server. The pairing code is single-use: reloading reconnects with a tab-scoped session token, and a second tab is refused until the backend restarts. WebSocket pings release an unresponsive session after a roughly 30-second pong timeout. The in-app Preview browser (a native child webview) and native dialogs do not render in the browser tab. `npm run dev:ui-fixture` runs Vite alone on port 1420 with the real (native) runtime: `tauri dev`'s `beforeDevCommand` and the native IDE journey use it, so their native window talks to the real backend. Playwright adds `--mode ui-fixture` itself to swap in the browser simulator for plain browser pages; nothing else needs that mode. A bare `tauri dev --features browser-bridge` (on your real profile) still opens the browser with a single-use link (printed instead with `SAGE_BRIDGE_PRINT_LINK=1`); until used, that link grants backend control, so do not paste it into logs or issues.

Native local gates (Windows, macOS, Linux):

```powershell
npm run gate:changed  # only what your diff from origin/main needs (see below)
npm run gate:static   # policy, prettier, eslint, tsc, rustfmt (about a minute)
npm run gate:fast     # static + Vitest + strict Clippy
npm run gate:rust     # three-phase Rust tests (parallel, then git-process and preview serially)
npm run gate:full     # fast + rust + budgets + build + bundle cap
npm run smoke         # headless rendered-app walkthrough with screenshots
npm run lane new X    # one-writer worktree for ticket X; `lane done X` integrates to main
```

`gate:changed` picks steps the way CI picks lanes (`scripts/change-scope.mjs`):
prettier and ESLint on the changed files, `tsc`, the Vitest files that import
them (every Vitest file when shared config such as `package.json` changes), the
Node budget tests when a script changed, and rustfmt, Clippy and the Rust test
phases only when the change touches a path the Rust crate depends on. Run
`gate:full` and the real-app checks before merging.

The Cargo target directory is a per-machine choice and is never tracked.
Either set `CARGO_TARGET_DIR` in the shell you build from, or create an
untracked `.cargo/config.toml` in the checkout (gitignored; TOML literal
string, so backslashes need no escaping):

```toml
[build]
target-dir = 'C:\Sage-build\target'
```

`npm run lane new <ticket>` writes that file into the new worktree pointing
at `C:\SageCaches\cargo-target\<ticket>`, and `lane done` uses the same
directory for its gate, so a ticket never shares a target with another
writer. The `npm run check:*` wrappers use their own `SAGE_CARGO_TARGET_DIR`
(see WORKFLOW.md). CI runners use Cargo's default.

`npm run lane -- done <ticket>` has two truthful integration outcomes. A
zero exit prints `cleanup complete` only after the exact lane commit has been
fast-forwarded into local `main`, publication succeeded when requested, every
recorded lane resource is absent, and the cleanup receipt has been removed. A
nonzero `LANE PARTIAL SUCCESS` means local integration already happened but a
later publication, receipt, or teardown stage did not finish; it reports the
full commit, push truth, failed stage, remaining states, and the exact command
to run again. After successful publication—or intentionally with `--no-push`—
the command first writes an atomic pending receipt at
`work/lane-receipts/<ticket>.json`, so rerunning the same `done` command enters
cleanup-only recovery without fetching, rebasing, gating, merging, or pushing
`main` again. `LANE CLEANUP BLOCKED` instead means the receipt or current Git /
filesystem authority is unsafe; it performs no destructive cleanup.

`--no-push` is explicitly local-only: its receipt records push requested and
succeeded as false, and cleanup neither inspects nor deletes a remote lane ref.
Neither local integration nor publication is CI, independent review, product
acceptance, or release evidence.

After the native prerequisites are installed:

```powershell
npm run tauri dev
cargo test --workspace
npm run gate:fast
```

`--all-features` does not build: `cdp-lane` is unix-only and `native-lane` is
Windows-only. The gate passes Clippy every feature that builds on your
platform (`scripts/cargo-features.mjs`).

## Project boundaries

The base installer must not grow a bundled Node runtime, Chromium, Monaco,
libgit2, a VS Code extension host, or built-in harness runtimes.

## License

Sage is licensed under the [Apache License, Version 2.0](LICENSE). See
[NOTICE](NOTICE) and [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

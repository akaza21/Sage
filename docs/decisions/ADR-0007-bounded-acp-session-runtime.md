# ADR-0007: Bounded ACP session runtime

Status: accepted.

Sage uses ACP as its first-class agent protocol. Harnesses run out of process;
Rust owns their lifecycle, session state and SQLite persistence. The WebView
renders disposable projections and submits typed commands. It does not own
durable transcripts or native process authority.

Every asynchronous operation carries its originating project, chat, session
and runtime ownership where applicable. Completion must revalidate that
ownership before changing visible state. Switching a project or chat does not
redirect an in-flight operation into a different owner's state.

Transport input, pending requests, events, retained transcript projections,
search results and attachment previews are bounded. Native walks stream and
fail closed at their limits. Workspace and Git walks admit at most 200,000
files, excluding `.git` and `node_modules`. Explorer displays at most 500
entries, Search at most 500 matches and Git at most 300 changes. A limit is
reported instead of silently presenting partial data as complete.

Harness selection determines the model catalog. Protocol behavior follows
negotiated capabilities; harness-specific installation metadata does not
become a protocol switch. Production Git operations use the Git CLI.

Agent and Editor views share workbench resources. Agent may show at most two
concurrent chat panes. Heavy inactive views unmount or sleep. Rendering
state, caches and lazy feature modules must not become durable truth.

Implementation boundaries live in `src-tauri/src`, `src/domain`, `src/ports`,
`src/services` and `src/features/chat`. Changes to these boundaries must retain
ownership checks, resource bounds and explicit failure behavior.

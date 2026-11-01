# ADR-0007: Bounded ACP session runtime

Status: Accepted.

Sage uses ACP as its first-class agent protocol. Harnesses run out of process;
selecting a harness determines which models and capabilities are available.

Rust and SQLite own durable session and workbench state. The WebView mirrors
that state for rendering and holds only disposable navigation, focus and layout
state. Project generations and native transition epochs fence asynchronous
results so an old session cannot update a replacement workspace.

Session transcripts, pending work, event delivery and file scans must remain
bounded. Persisted session events and recovery contracts live under
`docs/work-items/ADE-0006A-acp-session-foundation/contracts/`. Changes to those
contracts require updating their corresponding admission and recovery checks.

Agent mode admits at most two visible chat panes. Hidden heavy views sleep or
unmount. Opening Settings preserves workbench resources; returning through
history activates an existing chat without creating a chat or restoring a split.

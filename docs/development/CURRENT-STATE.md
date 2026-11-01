# Current state

Sage is a desktop agent workbench built with Tauri, Rust, SQLite, React and
TypeScript. Native code owns durable state, filesystem access and harness
processes. The WebView holds disposable presentation state.

The title bar selects Agent or Editor mode. Agent has project and chat
navigation on the left, one or two chat panes in the center, and the active
chat's resources on the right. Editor has its tool rail and a single File,
Diff or Settings tab strip. Files and diffs are shared workbench resources.

Chat uses ACP, bounded transcript projections and lazy feature modules.
Model catalogs come from the selected harness. Search and native file walks
enforce the limits documented in [ADR-0007](../decisions/ADR-0007-bounded-acp-session-runtime.md).

The GAP-B follow-up fixes user action visibility and keyboard access to model
search. Its browser checks exercise computed visual state and the production
popup. Cross-lane GAP-B14 and web-link GAP-B15 work remains incomplete; see
the [feature roadmap](FEATURE-ROADMAP.md).

Validation is recorded by running the individual scripts in `package.json`,
native formatting, Clippy and Rust tests, and the requested Playwright suite.
Passing local checks is not a claim of native visual acceptance or completion
of the outstanding cross-lane items.

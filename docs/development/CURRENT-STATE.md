# Current implementation boundaries

Sage has an Agent/Editor title-bar mode toggle. Agent mode shows project and
Chat navigation on the left, up to two visible Chat panes, and the focused
Chat's resources on the right. Editor mode uses its tool rail and a single
File/Diff/Settings tab strip. Both modes share workbench resources.

Rust and SQLite own durable project, Chat, and ACP data. The WebView owns
disposable rendering state. Harnesses run out of process; Git operations use
the Git CLI. Native walks and frontend lists remain bounded as documented
in `AGENTS.md` and the executable policy contracts.

Changed files open per-file Diff tabs. Saved agent edits retain immutable
snapshot references; live source is a separate File resource. Git staged,
unstaged, and HEAD comparisons have distinct tab identities, including after
native layout restoration.

See [feature delivery status](FEATURE-ROADMAP.md) for GAP-C remediation and
[delivery roadmap](ROADMAP.md) for the repository's delivery references.

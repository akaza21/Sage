# Current state

Sage has two window modes selected in the title bar. Agent mode provides project
and chat navigation, up to two visible chats, and chat-owned resources. Editor
mode provides Explorer, Search, Git, Problems and Outline beside a single
File / Diff / Settings tab strip. Both modes use the same workbench resources.

ACP harnesses run out of process. Harness discovery controls available models
and access modes. Rust and SQLite own durable truth; the WebView owns disposable
rendering state. Git CLI is the production Git backend.

Agent Settings is a nonmodal workspace page. Settings sections and individual
controls are searchable. Search availability follows the destination control's
supported state. Agent history is project-scoped and capped at 32 destinations.

See [feature delivery](FEATURE-ROADMAP.md) for the review fixes and
[delivery roadmap](ROADMAP.md) for the validation requirements. These public
documents describe supported boundaries; local gate logs are not release
attestations.

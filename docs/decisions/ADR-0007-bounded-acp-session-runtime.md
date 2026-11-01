# ADR-0007: Bounded ACP session runtime

Status: accepted.

ACP is the first-class agent protocol. Harness processes run outside Sage;
protocol behavior follows negotiated capabilities rather than harness names.
Rust validates ownership and admission and SQLite retains durable session,
turn, item, and immutable file-snapshot data. The WebView renders disposable
projections and must not become an authority for session ownership.

All transport, persistence, projection, and rendering paths have explicit
bounds. Admission fails closed when a request is stale, malformed, outside
its project roots, or over a limit. Current mutable actions retain live-owner
checks; historical file-snapshot reads require their exact retained identity
and the selected projection revision.

The executable contracts and numeric limits are maintained in
`docs/work-items/ADE-0006A-acp-session-foundation/contracts/`. Native ACP tests
cover ownership, retention, bounded transport, and snapshot reads; frontend
seam tests cover projection admission and resource rendering.

Consequences: harnesses cannot mutate durable truth through WebView state,
historical reads do not grant current execution authority, and inactive heavy
surfaces unmount or sleep. Git production operations use the Git CLI.

# Sage agent instructions

## Product invariants

- Chat is a workbench tab.
- Version 1 has one primary tab strip and no automatic splits.
- Project and persistent chat navigation live on the left.
- Explorer, Search, Git, Problems, and Outline live on the right.
- Agent edits appear inside the originating Chat as a bounded file summary.
- Selecting a changed file opens a per-file Diff tab; there is no dedicated
  Review tab.
- Explorer and Source Control follow the active File or Diff resource.
- ACP is the first-class agent protocol.
- Select harness first; derive models from that harness.
- Harnesses run out of process.
- Rust and SQLite own durable truth.
- The WebView owns disposable rendering state only.
- Git CLI will be the sole production Git backend.
- Worktrees are optional and never the project model.
- Heavy inactive views must unmount or sleep.

## Working rules

- Use one active production-code writer per checkout.
- Do not overwrite changes from another task.
- Keep production modules focused and ordinarily below 400 non-comment lines.
- Add tests for changed behavior.
- Run `npm run check` before handing off frontend or policy changes.
- Run Rust formatting, Clippy, and tests when a Rust toolchain is available.
- Do not add dependencies without documenting size and runtime impact.
- Do not create generic shell or filesystem IPC commands.
- Do not disable the Tauri CSP.
- Do not branch protocol behavior on harness names outside curated installation
  metadata.
- Never introduce unbounded queues, scans, terminal buffers, transcripts, or
  frontend lists.

## Default engineering flow

- Apply the implicit `engineering-workflow` skill to software work; the user
  does not need to invoke individual skills.
- Use the main flow as an index, entering at the earliest unresolved stage:
  `grill-with-docs` -> `to-spec` -> `to-tickets` -> `implement`, with `tdd`
  driving implementation and `code-review` closing it out.
- Scale the flow to the task. A small settled change may start at `tdd`; an
  agent-ready ticket starts at `implement`; a review request starts at
  `code-review`.
- Use `wayfinder` before the main flow only when the route is too uncertain or
  too large for one working session.
- Do not publish tracker artifacts, commit, push, or mutate external systems
  unless the user authorizes that action.

## Change handoff

Report:

- outcome;
- files and boundaries changed;
- tests and exact commands run;
- performance or bundle impact;
- known limitations;
- suggested review hotspots.

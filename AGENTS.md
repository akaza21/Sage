# Sage agent instructions

## Product invariants

- Chat is a workbench tab.
- Agent mode allows a bounded pair of side-by-side visible chats (max 2).
  That owner restatement supersedes “one primary tab strip and no automatic
  splits” for Agent chat panes only.
- Editor mode keeps one File / Diff / Settings tab strip and no automatic
  splits. Settings is the only non-resource tab admitted to this strip.
- The title bar owns a two-mode Agent / Editor window toggle (Devin-style
  D0). Do not reopen this decision.
- Agent view: project and persistent chat navigation live on the left; the
  center is one or two concurrent Chat panes. The right pane shows only that
  chat’s files, diffs, and in-app browser Preview. There is no Explorer /
  Search / Git / Problems / Outline chrome in Agent mode.
- Editor view is a pure editor: Explorer, Search, Git, Problems, and Outline
  live on the left tool rail; the center is the File / Diff / Settings tab
  strip. There is no project/chat navigation and no persistent
  chat-as-primary.
- Files, diffs, and previews opened in Agent are the same workbench
  resources as Editor mode.
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
- Workspace and Git file walks admit at most 200k files. WebView lists stay
  capped (Explorer 500, Search 500 matches, Git changes 300). Skip `.git`
  and `node_modules`. Native walks stream and fail closed; they do not ship
  200k paths to the WebView.
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

## Working from issues

Agent work is tracked as GitHub issues and shown on the project board.
Status labels are the source of truth for where a ticket stands:

- `status:ready`: specified and free to pick up.
- `status:in-progress`: an agent is working on it. Set this when you start.
- `status:in-review`: a PR is open. Link it with `Closes #<issue>` in the
  PR body.
- `status:blocked`: waiting on another PR or ticket; say which in a comment.

Owner labels say who does the work (`codex`, `claude`). `needs-owner` means
the maintainer must decide or act. `needs-real-app-test` means the change
must be checked in the running desktop app before merge.

When you take a ticket:

- Stay within the files the ticket lists. If the fix needs files outside
  that list, comment on the issue before widening the change.
- Open the PR as a draft, run the gates the ticket lists, and paste the
  exact commands and results into the PR.
- Mark the PR ready only when every acceptance check has evidence.
- Comment on the issue with what you delivered, what you could not verify,
  and any follow-up. Do not close the issue yourself; merging the PR does.
- Tests must lock in real behaviour: an input and its expected output, or a
  bug that stays fixed. Do not add tests that only check mocks, restate the
  implementation or snapshot trivial markup. Never skip, delete or loosen a
  test, or add retries, to make a gate pass.

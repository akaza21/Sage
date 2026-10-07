# Harness conformance traces

Each scenario is `<harness>/<scenario>/` and holds:

- `frames.jsonl` — sanitized real captured wire frames, one JSON object per
  line, `{ "direction": "in" | "out", "frame": { ... } }`. The raw originals
  are retained locally under ignored `local-evidence/`, never committed.
- `frames.jsonl.meta.json` — sanitized capture command, exit reason, tail of
  stderr.
- `expected.json` — the normalized `AdapterEvent` stream the contract in
  `docs/development/harness-adapter-contract.md` requires.

Every scenario directory must hold an `expected.json`. A trace may declare why
it cannot be asserted; it may never be hidden from the suite. The suite fails on
a missing contract, on a parked one (`expected.json.held` and friends), and on a
contract that asserts no events without declaring a `blocker` saying why.

Required keys:

- `handshake` — the outcome `StandardAcpRuntime` must reach when the captured
  responses are replayed through it: `session`, `signInRequired`,
  `initializeRejected`, `sessionRejected`, or one of the `no…Response` values.
  This is the only assertion available to a trace that never opens a session.
- `events` — the projected stream. `sessionReady` and `turn: running` entries are
  documentation; every other entry is asserted key by key. `chunks: N` repeats an
  entry, and `externalItemId: "$toolCallId"` resolves against the top-level
  `toolCallId`.
- `blocker` — required when the harness has no profile, and when `events` asserts
  nothing.

## Capture status

| Harness | Scenarios | Status |
|---|---|---|
| cursor-agent | `smoke`, `tool-lifecycle`, `tool-kinds` | captured, authenticated |
| opencode | `tool-kinds` | captured, authenticated; projects from a profile row with no bespoke translator |
| claude-code | `smoke`, `file-creation-permission` | captured, authenticated; `smoke` is a full turn through `end_turn`, `file-creation-permission` stops at `session/request_permission` and pins the nullable `oldText` a created file carries; both have the `_auth/status_update` account identity redacted |
| gemini-cli | `smoke` | handshake captured; `session/new` fails with `This account requires setting the GOOGLE_CLOUD_PROJECT env var`, so the contract asserts the refusal and an empty event stream |
| codex | — | pending, app-server is not ACP and needs its own capture driver |
| pi | — | pending, native rpc protocol needs its own capture driver |
| omp | — | pending |

## Unexercised by real traces

These surfaces are covered by unit tests only. No captured frame in this corpus
reaches them, so replaying every trace cannot falsify them and a regression here
will not show up as a failing conformance scenario.

- Server-to-client requests: `fs/read_text_file`, `fs/write_text_file` and the
  terminal methods. The capture client advertises `readTextFile: true`, yet no
  harness in this corpus has ever called back into it. The `clientRequest` branch
  of the translator and the `client_capabilities` rows in `profile.rs` are both
  unfalsifiable by replay.
- Live configuration mutation: `session/set_config_option` and the
  `config_option_update` notification. opencode's `session/new` response is the
  only captured configuration payload, and it is an initial snapshot.
- Options whose `type` is neither `select` nor `boolean`. Every captured option
  is a well-formed select, so the degrade-to-unsupported path is synthetic.

`scripts/capture-acp-trace.mjs` is local evidence tooling and is not
release-safe. Sanitize every frame and metadata path, retain raw originals
under ignored `local-evidence/`, and run `npm run check:fixture-privacy`
before admitting a recapture.

Recapture locally with:

```
node scripts/capture-acp-trace.mjs cursor-agent acp <out.jsonl> <cwd> "<prompt>"
```

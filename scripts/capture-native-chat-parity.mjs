// Waves 6-8 native capture catalogue: each Codex-parity state, the live Codex
// desktop frame it must match (docs: codex-live/sN-*/fNNNN-*.png, cited as
// "sN/fNNNN"), the parity-spec section, and per-harness admission. Data only:
// acp-native-cases.mjs, the capture DOM rules and the acceptance matrix all
// derive from this table, so they cannot disagree.

const SUPPORTED = "supported";
const absent = (reason) => Object.freeze({ absent: reason });
const gap = (reason) => Object.freeze({ gap: reason });

const PI_READ_ONLY = absent(
  "not projected or negotiated by the read-only Pi binding",
);
const NO_FORK_PI = absent(
  "Pi negotiates no nativeSessionFork; the answer menu is hidden",
);
const CLAUDE_NO_CHILD = absent(
  "Claude Task rows carry bounded metadata only; no per-child identity, list or child view",
);
const CLAUDE_NO_MCP = absent(
  "standard ACP has no MCP tool kind; claude-agent-acp MCP calls project as generic tool rows",
);
const SIDE_CHAT_RUNTIME = gap(
  "replay launch authority is one-shot per profile; a side-chat Turn needs a second harness runtime",
);
const ALL = Object.freeze({
  codex: SUPPORTED,
  claude: SUPPORTED,
  pi: SUPPORTED,
});

/**
 * `live`: the Codex live frames ("sN/fNNNN") the capture is compared with.
 * `spec`: the parity-spec section. `turns`: completed Turns the replay
 * answers (2 for states that send again). `widths`: admitted capture widths
 * when narrower panes hide the surface. `owner`: the Chat the capture
 * observes once driven ("capture" by default). `motion`: a frame burst.
 */
export const PARITY_STATES = Object.freeze([
  { state: "selection-popover", live: ["s2/f0093"], spec: "1", harnesses: ALL },
  { state: "annotation-field", live: ["s2/f0095"], spec: "2", harnesses: ALL },
  { state: "annotation-chip", live: ["s2/f0113"], spec: "2", harnesses: ALL },
  {
    state: "annotation-sent",
    live: ["s2/f0149"],
    spec: "2",
    harnesses: ALL,
    turns: 2,
  },
  {
    state: "side-chat-open",
    live: ["s2/f0244"],
    spec: "3",
    harnesses: ALL,
    owner: "side",
  },
  {
    state: "side-chat-running",
    live: ["s2/f0260"],
    spec: "3",
    harnesses: {
      codex: SIDE_CHAT_RUNTIME,
      claude: SIDE_CHAT_RUNTIME,
      pi: SIDE_CHAT_RUNTIME,
    },
    owner: "side",
  },
  {
    state: "side-chat-close-confirm",
    live: ["s2/f0352"],
    spec: "3",
    harnesses: ALL,
    owner: "side",
  },
  {
    state: "turn-navigator-hover",
    live: ["s6/f0651"],
    spec: "5",
    harnesses: ALL,
    turns: 2,
    // The rail is hidden below a 720 px chat pane (turn-navigator.css).
    widths: [1888],
  },
  { state: "find-in-chat", live: ["s6/f0676"], spec: "6", harnesses: ALL },
  {
    state: "tool-mcp-result",
    live: ["s5/f0390"],
    spec: "8",
    harnesses: { codex: SUPPORTED, claude: CLAUDE_NO_MCP, pi: PI_READ_ONLY },
  },
  {
    state: "tool-mcp-error",
    live: ["s5/f0397", "s5/f0405"],
    spec: "8",
    harnesses: { codex: SUPPORTED, claude: CLAUDE_NO_MCP, pi: PI_READ_ONLY },
  },
  {
    state: "tool-web-search",
    live: ["s5/f0405"],
    spec: "8",
    harnesses: { codex: SUPPORTED, claude: SUPPORTED, pi: PI_READ_ONLY },
  },
  {
    state: "tool-mixed-group",
    live: ["s5/f0378", "s2/f0015"],
    spec: "8",
    harnesses: {
      codex: SUPPORTED,
      claude: SUPPORTED,
      pi: absent(
        "the read-only Pi binding admits only read rows, so no mixed group forms",
      ),
    },
  },
  {
    state: "approval-card",
    live: ["s6/f0526"],
    spec: "9",
    harnesses: {
      codex: absent("Codex Ask is withdrawn per CP-5"),
      claude: SUPPORTED,
      pi: PI_READ_ONLY,
    },
  },
  {
    state: "subagents-name-hover",
    live: ["s4/f0485"],
    spec: "7",
    harnesses: { codex: SUPPORTED, claude: CLAUDE_NO_CHILD, pi: PI_READ_ONLY },
  },
  {
    state: "subagents-tab-active-done",
    live: ["s4/f0244"],
    spec: "7",
    harnesses: { codex: SUPPORTED, claude: CLAUDE_NO_CHILD, pi: PI_READ_ONLY },
  },
  {
    state: "subagent-child-header",
    live: ["s4/f0487"],
    spec: "7",
    harnesses: { codex: SUPPORTED, claude: CLAUDE_NO_CHILD, pi: PI_READ_ONLY },
  },
  {
    state: "fork-menu",
    live: ["s2/f0053"],
    spec: "4",
    harnesses: { codex: SUPPORTED, claude: SUPPORTED, pi: NO_FORK_PI },
  },
  {
    state: "fork-dialog",
    live: ["s2/f0055"],
    spec: "4",
    harnesses: { codex: SUPPORTED, claude: SUPPORTED, pi: NO_FORK_PI },
  },
  {
    state: "fork-thread",
    live: ["s2/f0066"],
    spec: "4",
    harnesses: { codex: SUPPORTED, claude: SUPPORTED, pi: NO_FORK_PI },
    owner: "fork",
  },
  {
    // §10.1 shimmers live labels; the cited frames sweep a running tool
    // label ("Running …"), which the replay holds live; Sage's live
    // reasoning prose is not a label and does not shimmer.
    state: "motion-thinking-shimmer",
    live: ["s8/f0130", "s8/f0175"],
    spec: "10.1",
    harnesses: ALL,
    motion: { mode: "wall", frames: 40, intervalMs: 100 },
  },
  {
    state: "motion-stream-fade",
    live: ["s5/f0299", "s5/f0309"],
    spec: "10.3",
    harnesses: ALL,
    motion: {
      mode: "seek",
      frames: 8,
      intervalMs: 50,
      animations: ["acp-stream-in"],
    },
  },
  {
    state: "motion-worked-expand",
    live: ["s2/f0011", "s2/f0015"],
    spec: "10.4",
    harnesses: ALL,
    motion: {
      mode: "seek",
      frames: 11,
      intervalMs: 60,
      animations: ["sage-row-in"],
      trigger: ".acp-turn:last-of-type .chat-transcript-work > summary",
    },
  },
  {
    state: "motion-jump-pill",
    live: ["s2/f0046", "s2/f0048", "s6/f0699"],
    spec: "10.7",
    harnesses: ALL,
    motion: {
      mode: "wall",
      frames: 16,
      intervalMs: 50,
      trigger: "button.chat-follow-end",
    },
  },
]);

const BY_STATE = new Map(PARITY_STATES.map((entry) => [entry.state, entry]));
export const PARITY_STATE_NAMES = Object.freeze([...BY_STATE.keys()]);
export const parityEntry = (state) => BY_STATE.get(state);
const harnessCell = (state, harness) => BY_STATE.get(state)?.harnesses[harness];

/** States this harness replays: supported cells only. */
export function paritySupported(harness) {
  return PARITY_STATE_NAMES.filter(
    (state) => harnessCell(state, harness) === SUPPORTED,
  );
}
export function parityAbsent(harness) {
  return PARITY_STATE_NAMES.filter(
    (state) => harnessCell(state, harness)?.absent,
  );
}
export function parityGaps(harness) {
  return PARITY_STATE_NAMES.filter((state) => harnessCell(state, harness)?.gap);
}
/** The truthful reason a cell is not captured, or null when it is. */
export function parityReason(state, harness) {
  const cell = harnessCell(state, harness);
  return cell === SUPPORTED || !cell ? null : (cell.absent ?? cell.gap);
}
/** Widths a state admits; every reference width unless narrowed. */
export function parityWidths(state) {
  return BY_STATE.get(state)?.widths ?? [1888, 720, 480];
}
export function parityMotion(state) {
  return BY_STATE.get(state)?.motion ?? null;
}
/** The Chat the capture observes after driving: capture, side or fork. */
export function parityOwner(state) {
  return BY_STATE.get(state)?.owner ?? "capture";
}
export function parityTurns(state) {
  return BY_STATE.get(state)?.turns ?? 1;
}
/** The matrix reference cell text: live frames plus spec section. */
export function parityReference(state) {
  const entry = BY_STATE.get(state);
  return entry
    ? "live " + entry.live.join(", ") + " (parity spec §" + entry.spec + ")"
    : null;
}
/** One acceptance-matrix cell, derived so the matrix and catalog agree. */
export function parityCell(state, harness) {
  const entry = BY_STATE.get(state);
  if (!entry) throw new Error("parity-state");
  const cell = entry.harnesses[harness];
  const refs = "refs " + parityReference(state);
  if (cell?.absent)
    return (
      "truthful-absent: " +
      cell.absent +
      "; surrounding-UI evidence pending; " +
      refs
    );
  if (cell?.gap) return "gap: " + cell.gap + "; " + refs;
  const motion = entry.motion
    ? "; " +
      entry.motion.mode +
      " frame burst " +
      entry.motion.frames +
      " x " +
      entry.motion.intervalMs +
      " ms"
    : "";
  const widths = entry.widths
    ? "width " + entry.widths.join("/") + " only; narrower panes hide it"
    : "widths 1888/720/480";
  return (
    "blocked: NC-4 native capture pending" +
    motion +
    "; " +
    refs +
    "; " +
    widths
  );
}
export function parityMatrixRow(state) {
  return (
    "| `" +
    state +
    "` | " +
    ["codex", "claude", "pi"]
      .map((harness) => parityCell(state, harness))
      .join(" | ") +
    " |"
  );
}

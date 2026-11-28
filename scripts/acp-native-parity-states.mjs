// Codex-parity Chat states and per-harness admission. Data only:
// acp-native-cases.mjs derives each harness's replay cases from this table.

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

/** `turns`: completed Turns the replay answers (2 for states that send again). */
const PARITY_STATES = Object.freeze([
  { state: "selection-popover", harnesses: ALL },
  { state: "annotation-field", harnesses: ALL },
  { state: "annotation-chip", harnesses: ALL },
  { state: "annotation-sent", harnesses: ALL, turns: 2 },
  { state: "side-chat-open", harnesses: ALL },
  {
    state: "side-chat-running",
    harnesses: {
      codex: SIDE_CHAT_RUNTIME,
      claude: SIDE_CHAT_RUNTIME,
      pi: SIDE_CHAT_RUNTIME,
    },
  },
  { state: "side-chat-close-confirm", harnesses: ALL },
  { state: "turn-navigator-hover", harnesses: ALL, turns: 2 },
  { state: "find-in-chat", harnesses: ALL },
  {
    state: "tool-mcp-result",
    harnesses: { codex: SUPPORTED, claude: CLAUDE_NO_MCP, pi: PI_READ_ONLY },
  },
  {
    state: "tool-mcp-error",
    harnesses: { codex: SUPPORTED, claude: CLAUDE_NO_MCP, pi: PI_READ_ONLY },
  },
  {
    state: "tool-web-search",
    harnesses: { codex: SUPPORTED, claude: SUPPORTED, pi: PI_READ_ONLY },
  },
  {
    state: "tool-mixed-group",
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
    harnesses: {
      codex: absent("Codex Ask is withdrawn per CP-5"),
      claude: SUPPORTED,
      pi: PI_READ_ONLY,
    },
  },
  {
    state: "subagents-name-hover",
    harnesses: { codex: SUPPORTED, claude: CLAUDE_NO_CHILD, pi: PI_READ_ONLY },
  },
  {
    state: "subagents-tab-active-done",
    harnesses: { codex: SUPPORTED, claude: CLAUDE_NO_CHILD, pi: PI_READ_ONLY },
  },
  {
    state: "subagent-child-header",
    harnesses: { codex: SUPPORTED, claude: CLAUDE_NO_CHILD, pi: PI_READ_ONLY },
  },
  {
    state: "fork-menu",
    harnesses: { codex: SUPPORTED, claude: SUPPORTED, pi: NO_FORK_PI },
  },
  {
    state: "fork-dialog",
    harnesses: { codex: SUPPORTED, claude: SUPPORTED, pi: NO_FORK_PI },
  },
  {
    state: "fork-thread",
    harnesses: { codex: SUPPORTED, claude: SUPPORTED, pi: NO_FORK_PI },
  },
  { state: "motion-thinking-shimmer", harnesses: ALL },
  { state: "motion-stream-fade", harnesses: ALL },
  { state: "motion-worked-expand", harnesses: ALL },
  { state: "motion-jump-pill", harnesses: ALL },
]);

const BY_STATE = new Map(PARITY_STATES.map((entry) => [entry.state, entry]));
const PARITY_STATE_NAMES = [...BY_STATE.keys()];
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
export function parityTurns(state) {
  return BY_STATE.get(state)?.turns ?? 1;
}

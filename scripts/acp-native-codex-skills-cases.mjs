// Codex skills replay journey. The replayed install holds repo skills, which
// skills/list names under each cwd Sage asks about, in Codex's SkillMetadata
// shape, with a disabled one Sage must not offer. A skill picked from the /
// menu reaches the turn as Codex's skill input item; each reply echoes the
// input the turn carried. After the first turn the skills change on disk:
// Codex says so with skills/changed, and the next read skips its cache.

const native = (method, item) => ({
  op: "emitKnownFrame",
  shape: "codexNative",
  method,
  params: { item },
});

const skill = (name, description, extra = {}) => ({
  name,
  description,
  path: `$ROOT/.codex/skills/${name}/SKILL.md`,
  scope: "repo",
  enabled: true,
  ...extra,
});

/** A name near the menu's bound, so the real-app check sees it fit. */
export const CODEX_LONG_SKILL =
  "review-release-notes-for-the-synthetic-example-project";

const INSTALLED = [
  skill("pdf", "Read and summarise PDF files", {
    interface: { shortDescription: "Summarise a PDF" },
  }),
  skill(CODEX_LONG_SKILL, "Check release notes against the changes"),
  skill("draft", "A disabled skill Sage must not list", { enabled: false }),
];

const CHANGED = [...INSTALLED, skill("changelog", "Write the changelog entry")];

const echoTurn = (id) => [
  { op: "awaitClient", kind: "codexTurnStart" },
  { op: "emitKnownFrame", shape: "codexTurnResult" },
  { op: "emitKnownFrame", shape: "codexStarted" },
  {
    op: "emitKnownFrame",
    shape: "codexNative",
    method: "item/agentMessage/delta",
    params: { itemId: id, delta: "$PROMPT_ECHO" },
  },
  native("item/completed", { type: "agentMessage", id, text: "$PROMPT_ECHO" }),
  { op: "emitKnownFrame", shape: "codexCompleted" },
];

export function codexSkillsJourneys() {
  return [
    {
      id: "codex-skills",
      steps: [
        { op: "awaitClient", kind: "codexInitialize" },
        {
          op: "emitKnownFrame",
          shape: "codexInitializeResultObserved",
          skills: INSTALLED,
        },
        { op: "awaitClient", kind: "codexStart" },
        { op: "emitKnownFrame", shape: "codexThreadResult" },
        { op: "awaitClient", kind: "codexGoalGetWithOptionalModelList" },
        { op: "emitKnownFrame", shape: "codexGoalMethodNotFound" },
        ...echoTurn("fixture-skill-echo-1"),
        { op: "emitKnownFrame", shape: "codexSkillsChanged", skills: CHANGED },
        ...echoTurn("fixture-skill-echo-2"),
        { op: "awaitClient", kind: "codexUnsubscribe" },
        { op: "emitKnownFrame", shape: "codexAck" },
      ],
    },
  ];
}

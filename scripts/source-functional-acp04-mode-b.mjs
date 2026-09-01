import {
  fail,
  rejectObservedFailures,
  validSelectIdentity,
  validateSnapshot,
  waitUntil,
} from "./source-functional-acp04-observation.mjs";
import { validateRuntimeObservation } from "./source-functional-acp04-finalizer.mjs";
import { codexSemanticVersion } from "./source-functional-acp04-live-contract.mjs";

const DECIMAL_REVISION = /^(?:0|[1-9]\d*)$/u;
const ACTIVE_TURN_STATES = new Set([
  "Queued",
  "Sending",
  "Running",
  "Waiting for model",
  "Waiting for tool",
  "Waiting for approval",
  "Waiting for user",
]);
const CANCELLED_TURN_STATES = new Set(["Cancelled", "Interrupted"]);

export const ACP04_MODE_B_CANCEL_PROMPT =
  "SAGE_ACP04_B_CANCEL: Produce a detailed 20-step implementation analysis, with at least 200 words per step. Start step 1 now and continue until explicitly cancelled.";
export const ACP04_MODE_B_RECOVERY_ECHO = "SAGE_ACP04_B_TURN_4";

export function createModeBCancelUi() {
  return Object.freeze({
    cancelEnabled:
      "(()=>{const panes=[...document.querySelectorAll('section.agent-chat-pane.agent-chat-pane--focused')];return panes.length===1&&panes[0].querySelectorAll('button[aria-label=\"Cancel turn\"]:not(:disabled)').length===1})()",
    cancel:
      "(()=>{const panes=[...document.querySelectorAll('section.agent-chat-pane.agent-chat-pane--focused')];if(panes.length!==1)return false;const x=[...panes[0].querySelectorAll('button[aria-label=\"Cancel turn\"]:not(:disabled)')];if(x.length!==1)return false;x[0].click();return true})()",
  });
}

function validateModeACompleteState(value) {
  if (
    !value ||
    value.phase !== "complete" ||
    !validSelectIdentity(value.harness) ||
    !validSelectIdentity(value.model) ||
    typeof value.projectId !== "string" ||
    value.projectId.length === 0 ||
    typeof value.chatId !== "string" ||
    value.chatId.length === 0 ||
    typeof value.sessionId !== "string" ||
    value.sessionId.length === 0 ||
    !DECIMAL_REVISION.test(value.runtimeEpoch ?? "") ||
    !DECIMAL_REVISION.test(value.projectionRevision ?? "") ||
    !Array.isArray(value.turnIds) ||
    value.turnIds.length !== 2 ||
    !Array.isArray(value.turnOrdinals) ||
    value.turnOrdinals.length !== 2 ||
    !Array.isArray(value.itemIds) ||
    value.itemIds.length < 4 ||
    new Set(value.turnIds).size !== value.turnIds.length ||
    new Set(value.itemIds).size !== value.itemIds.length ||
    !value.turnIds.every(
      (item) => typeof item === "string" && item.length > 0,
    ) ||
    !value.itemIds.every(
      (item) => typeof item === "string" && item.length > 0,
    ) ||
    !value.turnOrdinals.every(
      (item, index) =>
        Number.isSafeInteger(item) &&
        item >= 0 &&
        (index === 0 || item > value.turnOrdinals[index - 1]),
    )
  )
    fail("mode-a-state-invalid");
  return value;
}

function assertModeBStableIdentity(state, snapshot) {
  if (
    snapshot.projectId !== state.projectId ||
    snapshot.chatId !== state.chatId ||
    snapshot.currentSessionId !== state.sessionId ||
    snapshot.harness.id !== state.harness.id ||
    snapshot.harness.text !== state.harness.text ||
    snapshot.model.id !== state.model.id ||
    snapshot.model.text !== state.model.text ||
    snapshot.turns.some((turn) => turn.sessionId !== state.sessionId)
  )
    fail("mode-b-identity-churn");
  if (BigInt(snapshot.projectionRevision) < BigInt(state.projectionRevision))
    fail("projection-stale");
}

function assertRetainedModeBTurns(state, snapshot, count) {
  if (snapshot.turns.length < count) return false;
  if (snapshot.turns.length !== count) fail("mode-b-turn-cardinality");
  for (let index = 0; index < state.turnIds.length; index += 1) {
    if (
      snapshot.turns[index].id !== state.turnIds[index] ||
      snapshot.turns[index].ordinal !== state.turnOrdinals[index]
    )
      fail("mode-b-turn-churn");
  }
  const itemIds = snapshot.turns.flatMap((turn) =>
    turn.items.map((item) => item.id),
  );
  if (state.itemIds.some((itemId) => !itemIds.includes(itemId)))
    fail("mode-b-item-churn");
  return true;
}

function assertModeBThirdTurn(state, turn) {
  if (
    turn.id === state.turnIds[0] ||
    turn.id === state.turnIds[1] ||
    turn.ordinal <= state.turnOrdinals[1] ||
    turn.sessionId !== state.sessionId ||
    turn.runtimeEpoch !== state.runtimeEpoch ||
    turn.items.length < 1 ||
    turn.items[0].role !== "user" ||
    turn.items[0].text !== state.cancelPrompt ||
    turn.items.some((item) => state.itemIds.includes(item.id))
  )
    fail("mode-b-identity-reuse");
}

export function createModeBState(modeAState) {
  const modeA = validateModeACompleteState(modeAState);
  return Object.freeze({
    phase: "await-active-third",
    projectId: modeA.projectId,
    chatId: modeA.chatId,
    sessionId: modeA.sessionId,
    projectionRevision: modeA.projectionRevision,
    runtimeEpoch: modeA.runtimeEpoch,
    cancelRuntimeEpoch: modeA.runtimeEpoch,
    harness: { ...modeA.harness },
    model: { ...modeA.model },
    echoes: [...modeA.echoes],
    cancelPrompt: ACP04_MODE_B_CANCEL_PROMPT,
    recoveryEcho: ACP04_MODE_B_RECOVERY_ECHO,
    turnIds: [...modeA.turnIds],
    turnOrdinals: [...modeA.turnOrdinals],
    itemIds: [...modeA.itemIds],
    cancelledTurnId: null,
    cancelledState: null,
  });
}

export function acceptModeBSnapshot(state, value) {
  const snapshot = validateSnapshot(value);
  rejectObservedFailures(snapshot);
  assertModeBStableIdentity(state, snapshot);
  if (state.phase === "complete") return state;
  const expectedCount = state.phase === "await-fourth" ? 4 : 3;
  if (!assertRetainedModeBTurns(state, snapshot, expectedCount)) return state;
  const revision = BigInt(snapshot.projectionRevision);
  const priorRevision = BigInt(state.projectionRevision);
  if (state.phase === "await-active-third") {
    const third = snapshot.turns[2];
    assertModeBThirdTurn(state, third);
    if (!ACTIVE_TURN_STATES.has(third.state)) fail("mode-b-third-not-active");
    if (revision <= priorRevision) fail("projection-stale");
    return Object.freeze({
      ...state,
      phase: "await-terminal-third",
      projectionRevision: snapshot.projectionRevision,
      turnIds: [...state.turnIds, third.id],
      turnOrdinals: [...state.turnOrdinals, third.ordinal],
      itemIds: snapshot.turns.flatMap((turn) =>
        turn.items.map((item) => item.id),
      ),
    });
  }
  if (state.phase === "await-terminal-third") {
    const third = snapshot.turns[2];
    if (
      third.id !== state.turnIds[2] ||
      third.ordinal !== state.turnOrdinals[2] ||
      third.runtimeEpoch !== state.runtimeEpoch
    )
      fail("mode-b-cancel-terminal-invalid");
    if (ACTIVE_TURN_STATES.has(third.state) || third.state === "Cancelling")
      return state;
    if (!CANCELLED_TURN_STATES.has(third.state))
      fail("mode-b-cancel-terminal-invalid");
    if (revision <= priorRevision) fail("projection-stale");
    return Object.freeze({
      ...state,
      phase: "await-fourth",
      projectionRevision: snapshot.projectionRevision,
      itemIds: snapshot.turns.flatMap((turn) =>
        turn.items.map((item) => item.id),
      ),
      cancelledTurnId: third.id,
      cancelledState: third.state,
    });
  }
  const fourth = snapshot.turns[3];
  if (
    fourth.id === state.turnIds[0] ||
    fourth.id === state.turnIds[1] ||
    fourth.id === state.turnIds[2] ||
    fourth.ordinal <= state.turnOrdinals[2] ||
    fourth.runtimeEpoch === state.runtimeEpoch ||
    BigInt(fourth.runtimeEpoch) <= BigInt(state.runtimeEpoch)
  )
    fail("mode-b-runtime-restart-missing");
  if (fourth.state !== "Completed") {
    if (
      ["Failed", "Cancelled", "Interrupted", "Delivery unknown"].includes(
        fourth.state,
      )
    )
      fail("mode-b-recovery-terminal-failure");
    return state;
  }
  if (
    revision <= priorRevision ||
    fourth.items.length !== 2 ||
    fourth.items[0].role !== "user" ||
    fourth.items[0].text !== `Reply with exactly: ${state.recoveryEcho}` ||
    fourth.items[1].role !== "assistant" ||
    fourth.items[1].text !== state.recoveryEcho ||
    fourth.items.some((item) => state.itemIds.includes(item.id)) ||
    !["Ready", "Live"].includes(snapshot.harnessStatus) ||
    !snapshot.composerEnabled
  )
    fail("mode-b-recovery-invalid");
  return Object.freeze({
    ...state,
    phase: "complete",
    projectionRevision: snapshot.projectionRevision,
    runtimeEpoch: fourth.runtimeEpoch,
    turnIds: [...state.turnIds, fourth.id],
    turnOrdinals: [...state.turnOrdinals, fourth.ordinal],
    itemIds: snapshot.turns.flatMap((turn) =>
      turn.items.map((item) => item.id),
    ),
  });
}

export async function runAcp04ModeBWithDriver(
  deadline,
  modeAState,
  { driver, codexVersion },
) {
  if (!codexSemanticVersion(codexVersion)) fail("codex-version-invalid");
  const startedAt = new Date().toISOString();
  let state = createModeBState(modeAState);
  const observations = [];
  await driver.fillPrompt(state.cancelPrompt);
  await driver.waitForSendReady();
  await driver.send();
  const active = await waitUntil(
    async () => {
      const snapshot = await driver.snapshot();
      const next = acceptModeBSnapshot(state, snapshot);
      return next.phase === state.phase ? null : { next, snapshot };
    },
    deadline,
    "acp04-cancel-active-timeout",
  );
  state = active.next;
  observations.push(active.snapshot);
  await driver.waitForCancelReady();
  await driver.cancel();
  const cancelled = await waitUntil(
    async () => {
      const snapshot = await driver.snapshot();
      const next = acceptModeBSnapshot(state, snapshot);
      return next.phase === state.phase ? null : { next, snapshot };
    },
    deadline,
    "acp04-cancel-terminal-timeout",
  );
  state = cancelled.next;
  observations.push(cancelled.snapshot);
  await driver.fillPrompt(`Reply with exactly: ${state.recoveryEcho}`);
  await driver.waitForSendReady();
  await driver.send();
  const recovered = await waitUntil(
    async () => {
      const snapshot = await driver.snapshot();
      const next = acceptModeBSnapshot(state, snapshot);
      return next.phase === state.phase ? null : { next, snapshot };
    },
    deadline,
    "acp04-recovery-turn-timeout",
  );
  state = recovered.next;
  observations.push(recovered.snapshot);
  if (state.phase !== "complete") fail("mode-b-incomplete");
  const runtimeObservation = validateRuntimeObservation(
    await driver.runtimeObservation(),
    state,
  );
  if (runtimeObservation.admittedEntrypoint.version !== codexVersion)
    fail("runtime-version-mismatch");
  return Object.freeze({
    startedAt,
    completedAt: new Date().toISOString(),
    state,
    observations,
    runtimeObservation,
  });
}

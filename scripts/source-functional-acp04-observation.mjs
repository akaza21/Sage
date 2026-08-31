const FAILURE =
  /ProtocolFailure|HostError::Closed|ACP[^\n]{0,80}(?:recovery|capacity)/iu;
const DECIMAL_REVISION = /^(?:0|[1-9]\d*)$/u;

export function fail(code) {
  throw new Error(`acp04-${code}`);
}

export function validSelectIdentity(value) {
  return (
    value &&
    typeof value.id === "string" &&
    value.id.length > 0 &&
    typeof value.text === "string" &&
    value.text.length > 0
  );
}

function invalidSnapshotField(value) {
  if (!value || typeof value !== "object") return "root";
  if (typeof value.projectId !== "string" || value.projectId.length === 0)
    return "project-id";
  if (typeof value.chatId !== "string" || value.chatId.length === 0)
    return "chat-id";
  if (typeof value.currentSessionId !== "string") return "current-session-id";
  if (!DECIMAL_REVISION.test(value.projectionRevision ?? ""))
    return "projection-revision";
  if (!validSelectIdentity(value.harness)) return "harness";
  if (!validSelectIdentity(value.model)) return "model";
  if (typeof value.harnessStatus !== "string") return "harness-status";
  if (typeof value.composerEnabled !== "boolean") return "composer-enabled";
  if (
    !Array.isArray(value.failures) ||
    !value.failures.every((item) => typeof item === "string")
  )
    return "failures";
  if (!Array.isArray(value.turns)) return "turns";
  return null;
}

export function validateSnapshot(value) {
  const validTurn = (turn) =>
    turn &&
    typeof turn.id === "string" &&
    turn.id.length > 0 &&
    typeof turn.sessionId === "string" &&
    turn.sessionId.length > 0 &&
    DECIMAL_REVISION.test(turn.runtimeEpoch ?? "") &&
    Number.isSafeInteger(turn.ordinal) &&
    turn.ordinal > 0 &&
    typeof turn.state === "string" &&
    turn.state.length > 0 &&
    Array.isArray(turn.items) &&
    turn.items.every(
      (item) =>
        item &&
        typeof item.id === "string" &&
        item.id.length > 0 &&
        ["user", "assistant", "system"].includes(item.role) &&
        typeof item.text === "string",
    );
  const invalidField = invalidSnapshotField(value);
  if (invalidField) fail(`snapshot-invalid:${invalidField}`);
  if (!value.turns.every(validTurn)) fail("snapshot-invalid:turn");
  const turnIds = value.turns.map((turn) => turn.id);
  const itemIds = value.turns.flatMap((turn) =>
    turn.items.map((item) => item.id),
  );
  if (
    new Set(turnIds).size !== turnIds.length ||
    new Set(itemIds).size !== itemIds.length ||
    value.turns.some(
      (turn, index) =>
        index > 0 && turn.ordinal <= value.turns[index - 1].ordinal,
    )
  )
    fail("snapshot-invalid:identity-order");
  return value;
}

export function rejectObservedFailures(snapshot) {
  if (
    snapshot.failures.length > 0 ||
    snapshot.failures.some((item) => FAILURE.test(item))
  )
    fail("observed-failure");
}

export async function waitUntil(action, deadline, message) {
  while (Date.now() < deadline) {
    const result = await action();
    if (result) return result;
    await new Promise((resolve) =>
      setTimeout(resolve, Math.min(100, Math.max(0, deadline - Date.now()))),
    );
  }
  throw new Error(message);
}

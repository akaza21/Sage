export const POLL_INTERVAL_MS = 50;
export const CDP_OPERATION_CAP_MS = 10_000;
export const PROBE_MARGIN_MS = 250;

export function pollDelay(until, now, interval = POLL_INTERVAL_MS) {
  if (!Number.isFinite(until) || !Number.isFinite(now)) return 0;
  return Math.max(0, Math.min(interval, until - now));
}

export function probeExpiry(
  until,
  now,
  cap = CDP_OPERATION_CAP_MS,
  margin = PROBE_MARGIN_MS,
) {
  if (!Number.isFinite(until) || !Number.isFinite(now)) return now;
  return Math.max(now, Math.min(until, now + cap) - margin);
}

export async function boundedRead(read, options) {
  const settled = read().then(
    (value) => ({ kind: "value", value }),
    (error) => ({ kind: "error", error }),
  );
  return Promise.race([
    settled,
    options.expire().then(() => ({ kind: "unresponsive" })),
  ]);
}

export async function freshRead(read, options) {
  const value = await read();
  if (options.now() >= options.until) return { kind: "late", value };
  return { kind: "value", value };
}

export function harnessReadiness(state) {
  if (state?.status === "Ready") return state.composerDisabled === false;
  return [
    "Sign in required",
    "Sign-in consent",
    "Waiting for sign in",
    "Unavailable",
  ].includes(state?.status);
}

export function harnessPickerReadiness(state) {
  if (state?.alertCount > 0 || state?.alertsExceeded !== false) return "error";
  if (!state?.triggerPresent || state.projectLoading) return "transitioning";
  return state.triggerDisabled ? "persistent" : "ready";
}

export async function pollReadiness(probe, classify, options) {
  let state = await probe();
  for (;;) {
    if (options.now() >= options.until) return { status: "timeout", state };
    if (state == null) return { status: "scopeLost", state };
    const status = classify(state);
    if (status !== "transitioning") return { status, state };
    await options.yieldFrame();
    if (options.now() >= options.until) return { status: "timeout", state };
    state = await probe();
  }
}

export function composerReady(state, prompt) {
  return state?.value === prompt && state.disabled === false;
}

function gainedTurn(initialPane, currentPane) {
  return (
    Number.isInteger(initialPane?.turnCount) &&
    Number.isInteger(currentPane?.turnCount) &&
    currentPane.turnCount > initialPane.turnCount
  );
}

function raisedAlert(initialPane, currentPane) {
  return (
    initialPane?.alertClass !== "present" &&
    currentPane?.alertClass === "present"
  );
}

export function turnAcceptanceOutcome(initial, sample) {
  if (gainedTurn(initial?.right, sample?.right)) return "accepted";
  if (gainedTurn(initial?.left, sample?.left)) return "routed-left";
  if (
    raisedAlert(initial?.left, sample?.left) ||
    raisedAlert(initial?.right, sample?.right)
  )
    return "rejected-alert";
  if (sample?.focusedPane !== initial?.expectedPane) return "focus-drift";
  if (
    sample?.right?.composerHasValue === true &&
    sample?.right?.composerDisabled === true
  )
    return "activation-stalled";
  if (sample?.right?.composerHasValue === false) return "prompt-stalled";
  return "not-dispatched";
}

export async function pollTurnAcceptance(probe, initial, options) {
  const samples = [];
  const maximumSamples = options.maximumSamples ?? 101;
  let previous = null;
  for (;;) {
    const sample = await probe();
    const encoded = JSON.stringify(sample);
    if (encoded !== previous) {
      if (samples.length >= maximumSamples)
        return { outcome: turnAcceptanceOutcome(initial, sample), samples };
      samples.push(sample);
      previous = encoded;
    }
    const outcome = turnAcceptanceOutcome(initial, sample);
    if (
      ["accepted", "routed-left", "rejected-alert"].includes(outcome) ||
      options.now() >= options.until
    )
      return { outcome, samples };
    await options.wait();
  }
}

export function submissionReadiness(state) {
  if (state?.alerts?.length > 0 || state?.alertsExceeded !== false)
    return "error";
  if (state?.disabled === false) return "ready";
  if (
    [
      "Sign in required",
      "Sign-in consent",
      "Waiting for sign in",
      "Unavailable",
    ].includes(state?.status)
  )
    return "persistent";
  return "transitioning";
}

export function turnIdsPresent(expectedTurnIds, currentTurnIds) {
  if (!Array.isArray(expectedTurnIds) || expectedTurnIds.length === 0)
    return false;
  if (!Array.isArray(currentTurnIds)) return false;
  const current = new Set(currentTurnIds);
  return expectedTurnIds.every(
    (turnId) =>
      typeof turnId === "string" && turnId.length > 0 && current.has(turnId),
  );
}

export function discoveryReady(options) {
  return (
    Array.isArray(options) &&
    options.length > 0 &&
    options.every(
      (option) => !/(?:checking|detecting)/iu.test(option?.text ?? ""),
    )
  );
}

export function turnSettled(state) {
  return state?.composerDisabled === false && state.cancelVisible === false;
}

export function scanExceeded(state, turn) {
  return (
    state?.turnsExceeded !== false ||
    state?.alertsExceeded !== false ||
    turn?.toolsExceeded !== false
  );
}

export function turnOutcome(
  state,
  turnId,
  cancellationRequested,
  expectedReply,
) {
  const matched = Array.isArray(state?.turns)
    ? state.turns.filter((candidate) => candidate?.id === turnId)
    : [];
  if (matched.length > 1) return { kind: "turn-ambiguous" };
  const turn = matched[0];
  if (!turn || typeof turn.status !== "string") return null;
  if (state?.turnsExceeded !== false || state?.alertsExceeded !== false)
    return { kind: "scan-exceeded" };
  if (state.alerts?.length > 0) return { kind: "error" };
  if (cancellationRequested) {
    if (turn.status === "Interrupted" && turnSettled(state))
      return { kind: "interrupted" };
    if (turn.status === "Cancelled" && turnSettled(state)) {
      if (turn.reason === "cancelledByUser") return { kind: "cancelled" };
      return { kind: "cancel-refused" };
    }
    if (["Completed", "Failed"].includes(turn.status))
      return { kind: "cancel-refused" };
    return null;
  }
  if (turn.status === "Completed" && turnSettled(state)) {
    if (expectedReply && turn.assistantTextTruncated !== false)
      return { kind: "response-mismatch" };
    if (
      expectedReply &&
      !expectedReply.test(String(turn.assistantText ?? "").trim())
    )
      return { kind: "response-mismatch" };
    return { kind: "completed" };
  }
  if (["Interrupted", "Cancelled", "Failed"].includes(turn.status))
    return { kind: "error" };
  return null;
}

import { waitForState } from "./capture-native-chat-contract.mjs";
import { SCROLL_TO_START } from "./capture-native-chat-pane.mjs";
import {
  liveTurnReady,
  liveActionExpression,
  SETTLED_TURN_STATES,
  composerTurnControlReady,
  COMPOSER_FILL,
  SEND_MESSAGE,
} from "./capture-native-chat-dom.mjs";

/**
 * One owned Turn's steps: submit it, act on it while live, and wait for it to
 * settle. The capture driver re-exports all of these for its importers.
 */
export async function submitTurn(
  evaluate,
  observationExpression,
  clickCapture,
  bindingId,
  deadline,
  record,
) {
  const prior = await evaluate(observationExpression);
  record("composer-input-start", prior);
  await waitForState(() => evaluate(COMPOSER_FILL), deadline);
  record("send-start", prior);
  await clickCapture(SEND_MESSAGE);
  record("send-clicked", prior);
  let submitted;
  await waitForState(async () => {
    submitted = await evaluate(observationExpression);
    return (
      submitted.turnCount > prior.turnCount &&
      submitted.turnId !== prior.turnId &&
      submitted.bindingId === bindingId
    );
  }, deadline);
  record("turn-observed", submitted);
  return submitted.turnId;
}
export async function waitForComposerIdle(
  evaluate,
  observationExpression,
  turnId,
  bindingId,
  deadline,
  record,
  control = "modelEnabled",
) {
  let latest = await evaluate(observationExpression);
  record("composer-idle-start", latest);
  await waitForState(async () => {
    latest = await evaluate(observationExpression);
    if (latest.turnId !== turnId || latest.bindingId !== bindingId)
      throw new Error("turn-identity-drift");
    return (
      SETTLED_TURN_STATES.includes(latest.state) &&
      !latest.controls.stop &&
      composerTurnControlReady(latest.controls) &&
      latest.controls[control] === true
    );
  }, deadline);
  record("composer-idle-ready", latest);
  return latest;
}
export async function scrollCaptureTranscript(
  evaluate,
  observationExpression,
  turnId,
  bindingId,
  deadline,
) {
  await waitForState(async () => {
    const latest = await evaluate(observationExpression);
    if (latest.turnId !== turnId || latest.bindingId !== bindingId)
      throw new Error("turn-identity-drift");
    if (latest.state !== "completed") return false;
    if (!(await evaluate(SCROLL_TO_START))) return false;
    return (await evaluate(observationExpression)).visible.jump === true;
  }, deadline);
}
export async function clickReadyTurn(
  evaluate,
  observationExpression,
  selector,
  turnId,
  bindingId,
  deadline,
  record,
) {
  let latest;
  try {
    latest = await evaluate(observationExpression);
    record("turn-readiness-start", latest);
    await waitForState(async () => {
      latest = await evaluate(observationExpression);
      if (latest.turnId !== turnId || latest.bindingId !== bindingId)
        throw new Error("turn-identity-drift");
      if (SETTLED_TURN_STATES.includes(latest.state))
        throw new Error("turn-action-settled");
      return liveTurnReady(latest, turnId, bindingId);
    }, deadline);
    record("turn-ready", latest);
    record("turn-action-start", latest);
    await waitForState(async () => {
      const clicked = await evaluate(
        liveActionExpression(
          observationExpression,
          selector,
          turnId,
          bindingId,
        ),
      );
      if (["turn-action-settled", "turn-identity-drift"].includes(clicked))
        throw new Error(clicked);
      if (!clicked) latest = await evaluate(observationExpression);
      return clicked === true;
    }, deadline);
    record("turn-action-clicked", latest);
  } catch (error) {
    if (
      ["turn-action-settled", "turn-identity-drift"].includes(error.message)
    ) {
      try {
        latest = await evaluate(observationExpression);
      } catch {
        record("turn-observation-blocked", latest);
      }
    }
    record("turn-action-blocked", latest);
    if (error.message === "state-timeout")
      throw new Error("turn-readiness-timeout", { cause: error });
    throw error;
  }
}

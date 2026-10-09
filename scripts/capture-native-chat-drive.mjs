import { waitForState } from "./capture-native-chat-contract.mjs";
import {
  observationFor,
  HARNESS,
  freshEmptyChat,
  clickExpression,
  liveTurnReady,
  liveActionExpression,
  SETTLED_TURN_STATES,
} from "./capture-native-chat-dom.mjs";

export async function captureStep(record, step, action) {
  record(step + "-start");
  try {
    const result = await action();
    record(step + "-finished");
    return result;
  } catch (error) {
    record(step + "-failed");
    throw error;
  }
}

export async function prepareCaptureChat(
  cdp,
  harness,
  deadline,
  record = () => {},
) {
  const OBSERVATION = observationFor(HARNESS[harness].binding);
  const observe = () =>
    cdp.evaluate(OBSERVATION, Math.min(deadline, Date.now() + 5000));
  let previous;
  await waitForState(async () => {
    const observation = await observe();
    if (
      !observation.chatId ||
      !observation.projectId ||
      observation.projectionLoaded !== true
    )
      return false;
    previous = observation;
    return true;
  }, deadline);
  record("fresh-chat-start", previous);
  const compact = await cdp.evaluate(
    "Boolean(document.querySelector('button[aria-label=\"Open projects and chats\"]'))",
    Math.min(deadline, Date.now() + 5000),
  );
  if (compact)
    await click(
      cdp,
      'button[aria-label="Open projects and chats"]',
      deadline,
      record,
    );
  await click(cdp, 'button[aria-label="New task"]', deadline, record);
  await waitForState(
    async () => freshEmptyChat(previous, await observe()),
    deadline,
  );
  record("fresh-chat-ready", await observe());
  if (compact)
    await click(
      cdp,
      'button[aria-label="Close projects and chats"]',
      deadline,
      record,
    );
  await click(cdp, 'button[aria-label="Harness"]', deadline, record);
  await click(
    cdp,
    '[role="option"][data-option-id="' + HARNESS[harness].id + '"]',
    deadline,
    record,
  );
  await waitForState(
    async () => (await observe()).selected === HARNESS[harness].id,
    deadline,
  );
  record("harness-selected", await observe());
}
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
  await waitForState(
    () =>
      evaluate(
        '(()=>{const input=document.querySelector(\'textarea[aria-label="Message agent"]\');if(!input || input.disabled)return false;Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype,"value").set.call(input,"Inspect the fixed synthetic native replay example.");input.dispatchEvent(new Event("input",{bubbles:true}));return true})()',
      ),
    deadline,
  );
  record("send-start", prior);
  await clickCapture('button[aria-label="Send message"]');
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
      latest.controls.send === true &&
      latest.controls.modelEnabled === true
    );
  }, deadline);
  record("composer-idle-ready", latest);
  return latest;
}
export function retainCaptureError(primary, next) {
  if (!primary) return next;
  if (next?.cleanupError && !primary.cleanupError)
    primary.cleanupError = next.cleanupError;
  return primary;
}
export async function cleanupCapture(record, steps) {
  if (steps.length > 4) throw new Error("cleanup-bound");
  let failure;
  const note = (step) => {
    try {
      record(step);
    } catch (error) {
      failure = retainCaptureError(failure, error);
    }
  };
  for (const { step, action } of steps) {
    if (!action) continue;
    note(step + "-start");
    try {
      await action();
      note(step + "-finished");
    } catch (error) {
      failure = retainCaptureError(failure, error);
      note(step + "-failed");
    }
  }
  return failure;
}
export async function click(cdp, selector, deadline, record) {
  await captureStep(record, "control-click", () =>
    waitForState(
      () =>
        cdp.evaluate(
          clickExpression(selector),
          Math.min(deadline, Date.now() + 5000),
        ),
      deadline,
    ),
  );
}
export async function stableLayout(cdp, deadline, OBSERVATION) {
  await cdp.evaluate(
    "(async()=>{await document.fonts.ready;await new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)));return true})()",
    Math.min(deadline, Date.now() + 10000),
  );
  let prior = null,
    consecutive = 0;
  await waitForState(async () => {
    const observation = await cdp.evaluate(
      OBSERVATION,
      Math.min(deadline, Date.now() + 5000),
    );
    const signature = JSON.stringify([
      observation.css,
      observation.elements.map(({ kind, x, y, width, height }) => ({
        kind,
        x,
        y,
        width,
        height,
      })),
    ]);
    consecutive = signature === prior ? consecutive + 1 : 0;
    prior = signature;
    return consecutive >= 3;
  }, deadline);
}
export function captureTiming() {
  const steps = [];
  const record = (step, observation) => {
    if (steps.length >= 64) throw new Error("timing-bound");
    steps.push({
      step,
      atMs: Date.now(),
      turnId: observation?.turnId ?? "none",
      state: observation?.state ?? "none",
      cancelPresent: observation?.controls?.stop === true,
      assistantRendered: observation?.assistantRendered === true,
    });
  };
  return { steps, record };
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

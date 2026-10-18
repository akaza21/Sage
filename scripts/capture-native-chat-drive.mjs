import { waitForState } from "./capture-native-chat-contract.mjs";
import {
  boundedEvaluate,
  retainCaptureError,
} from "./capture-native-chat-shared.mjs";
import {
  archivePreviousChatExpression,
  SINGLE_CAPTURE_PANE,
  SCROLL_TO_START,
} from "./capture-native-chat-pane.mjs";
import {
  observationFor,
  HARNESS,
  freshEmptyChat,
  clickExpression,
  liveTurnReady,
  liveActionExpression,
  SETTLED_TURN_STATES,
  IDLE_CAPTURE_STATES,
  actionsFor,
  isLiveTurnAction,
  scopeToTurn,
  isDisclosure,
  openExpression,
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
  const evaluate = boundedEvaluate(cdp, deadline);
  const observe = () => evaluate(OBSERVATION);
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
  const compact = await evaluate(
    "Boolean(document.querySelector('button[aria-label=\"Open projects and chats\"]'))",
  );
  if (compact)
    await click(
      cdp,
      'button[aria-label="Open projects and chats"]',
      deadline,
      record,
    );
  await click(cdp, 'button[aria-label="New task"]', deadline, record);
  let captureChat;
  await waitForState(async () => {
    const observation = await observe();
    if (!freshEmptyChat(previous, observation)) return false;
    captureChat = {
      chatId: observation.chatId,
      projectId: observation.projectId,
    };
    return true;
  }, deadline);
  if (!(await evaluate(archivePreviousChatExpression(previous.chatId))))
    throw new Error("capture-chat-archive-failed");
  await waitForState(() => evaluate(SINGLE_CAPTURE_PANE), deadline);
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
  return captureChat;
}
export async function driveCaptureChat(cdp, options, deadline, record) {
  const OBSERVATION = observationFor(HARNESS[options.harness].binding);
  const evaluate = boundedEvaluate(cdp, deadline);
  const clickCapture = (selector) => click(cdp, selector, deadline, record);
  const captureChat = await prepareCaptureChat(
    cdp,
    options.harness,
    deadline,
    record,
  );
  let submittedTurnId;
  const submit = async () => {
    submittedTurnId = await submitTurn(
      evaluate,
      OBSERVATION,
      clickCapture,
      HARNESS[options.harness].binding,
      deadline,
      record,
    );
  };
  if (options.state.startsWith("approval-")) {
    const mode = (await evaluate(OBSERVATION)).accessModes?.find(
      (mode) => mode === "ask",
    );
    if (!mode) throw new Error("approval-access-unavailable");
    await clickCapture('button[aria-label="Access options"]');
    await clickCapture('[role="menuitemradio"][data-option-id="' + mode + '"]');
    await submit();
    await waitForState(
      async () => (await evaluate(OBSERVATION)).state === "completed",
      deadline,
    );
  }
  if (!IDLE_CAPTURE_STATES.includes(options.state)) await submit();
  if (options.state === "jump-to-latest")
    await scrollCaptureTranscript(
      evaluate,
      OBSERVATION,
      submittedTurnId,
      HARNESS[options.harness].binding,
      deadline,
    );
  if (["model-menu", "effort-menu"].includes(options.state))
    await waitForComposerIdle(
      evaluate,
      OBSERVATION,
      submittedTurnId,
      HARNESS[options.harness].binding,
      deadline,
      record,
      options.state === "effort-menu" ? "effortEnabled" : "modelEnabled",
    );
  for (const action of actionsFor(options.state, options.harness)) {
    let latest = await evaluate(OBSERVATION);
    if (isLiveTurnAction(action)) {
      await clickReadyTurn(
        evaluate,
        OBSERVATION,
        scopeToTurn(action, submittedTurnId),
        submittedTurnId,
        HARNESS[options.harness].binding,
        deadline,
        record,
      );
      continue;
    }
    record("action-start", latest);
    if (!isDisclosure(action)) {
      await clickCapture(scopeToTurn(action, latest.turnId));
      record("action-clicked", latest);
      continue;
    }
    await waitForState(async () => {
      latest = await evaluate(OBSERVATION);
      return SETTLED_TURN_STATES.includes(latest.state);
    }, deadline);
    const selector = scopeToTurn(action, latest.turnId);
    await waitForState(() => evaluate(openExpression(selector)), deadline);
    record("disclosure-opened", latest);
  }
  return captureChat;
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
        '(()=>{const input=document.querySelector(\'.agent-chat-pane--focused textarea[aria-label="Message agent"]\');if(!input || input.disabled)return false;Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype,"value").set.call(input,"Inspect the fixed synthetic native replay example.");input.dispatchEvent(new Event("input",{bubbles:true}));return true})()',
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
      latest.controls.send === true &&
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
  const evaluate = boundedEvaluate(cdp, deadline);
  await captureStep(record, "control-click", () =>
    waitForState(() => evaluate(clickExpression(selector)), deadline),
  );
}
export async function stableLayout(cdp, deadline, OBSERVATION) {
  const evaluate = boundedEvaluate(cdp, deadline);
  await cdp.evaluate(
    "(async()=>{await document.fonts.ready;await new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)));return true})()",
    Math.min(deadline, Date.now() + 10000),
  );
  let prior = null,
    consecutive = 0;
  await waitForState(async () => {
    const observation = await evaluate(OBSERVATION);
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
      lazyFallbackVisible: observation?.lazyFallbackVisible === true,
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

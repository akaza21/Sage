// Drives the waves 6-8 parity states after the first Turn was submitted.
// Selection and hover use real CDP pointer input (a synthetic DOM event can
// neither make a native selection nor set :hover); Ctrl+F is a real key.
import { waitForState } from "./capture-native-chat-contract.mjs";
import { paneSelector, ownedPaneNodes } from "./capture-native-chat-pane.mjs";
import {
  SETTLED_TURN_STATES,
  clickExpression,
  openExpression,
  scopeToTurn,
} from "./capture-native-chat-dom.mjs";
import {
  parityStepsFor,
  phraseRectExpression,
} from "./capture-native-chat-parity-dom.mjs";

const SCOPES = new Set(["pane", "document", "owner"]);
const DRAG_STEPS = 8;

function nodesExpression(selector, scope, chatId) {
  if (!SCOPES.has(scope)) throw new Error("parity-scope");
  if (scope === "document")
    return `(()=>{const nodes=[...document.querySelectorAll(${JSON.stringify(selector)})];if(nodes.length>64)throw Error('capture-owner-bound');return nodes})()`;
  return `(()=>{const paneSelector=${paneSelector.toString()};const ownedPaneNodes=${ownedPaneNodes.toString()};return ownedPaneNodes(document,${JSON.stringify(selector)},${JSON.stringify(scope === "owner" ? chatId : null)})})()`;
}

/** Clicks the single enabled match in the named scope; true once clicked. */
export function scopedClickExpression(selector, scope, chatId) {
  if (scope === "pane" || scope === "owner")
    return clickExpression(selector, scope === "owner" ? chatId : null);
  return `(()=>{const nodes=${nodesExpression(selector, scope, chatId)};if(nodes.length!==1 || nodes[0].disabled)return false;nodes[0].click();return true})()`;
}

/** Sets one input's value through the native setter so React sees it. */
export function fillExpression(selector, scope, text, chatId) {
  if (typeof text !== "string" || text.length > 256)
    throw new Error("parity-fill");
  return `(()=>{const nodes=${nodesExpression(selector, scope, chatId)};const input=nodes[0];if(nodes.length!==1 || input.disabled || !(input instanceof HTMLInputElement || input instanceof HTMLTextAreaElement))return false;input.focus();const proto=input instanceof HTMLTextAreaElement?HTMLTextAreaElement.prototype:HTMLInputElement.prototype;Object.getOwnPropertyDescriptor(proto,'value').set.call(input,${JSON.stringify(text)});input.dispatchEvent(new Event('input',{bubbles:true}));return input.value===${JSON.stringify(text)}})()`;
}

/** The centre of the single visible match, for real pointer input. */
export function centerExpression(selector, scope, chatId) {
  return `(()=>{const nodes=${nodesExpression(selector, scope, chatId)};if(nodes.length!==1)return false;nodes[0].scrollIntoView({block:'nearest',inline:'nearest',behavior:'instant'});const box=nodes[0].getBoundingClientRect();if(!(box.width>0&&box.height>0)||box.x<0||box.y<0||box.x+box.width>innerWidth||box.y+box.height>innerHeight)return false;return {x:box.x+box.width/2,y:box.y+box.height/2}})()`;
}

export function focusExpression(selector, chatId) {
  return `(()=>{const nodes=${nodesExpression(selector, "pane", chatId)};if(nodes.length!==1 || nodes[0].disabled)return false;nodes[0].focus();return document.activeElement===nodes[0]})()`;
}

async function located(evaluate, expression, deadline) {
  let value;
  await waitForState(async () => {
    value = await evaluate(expression);
    return Boolean(value);
  }, deadline);
  return value;
}

/** A left-button drag across the phrase, as a reader selects answer text. */
export async function dragSelect(cdp, rect, deadline) {
  const y = rect.y + rect.height / 2;
  const at = Math.min(deadline, Date.now() + 5000);
  await cdp.mouse({ type: "mouseMoved", x: rect.x + 1, y }, at);
  await cdp.mouse(
    { type: "mousePressed", x: rect.x + 1, y, button: "left" },
    at,
  );
  for (let step = 1; step <= DRAG_STEPS; step++)
    await cdp.mouse(
      {
        type: "mouseMoved",
        x: rect.x + 1 + (rect.width * step) / DRAG_STEPS,
        y,
        button: "left",
      },
      at,
    );
  await cdp.mouse(
    { type: "mouseReleased", x: rect.x + rect.width + 1, y, button: "left" },
    at,
  );
}

/**
 * Runs the state's parity steps. `submit` sends a further Turn and returns
 * its identity; every Turn-scoped selector is pinned to the latest Turn.
 */
export async function driveParitySteps({
  cdp,
  evaluate,
  observe,
  state,
  captureChat,
  submittedTurnId,
  submit,
  deadline,
  record,
}) {
  let turnId = submittedTurnId;
  const settled = async () => {
    let latest;
    await waitForState(async () => {
      latest = await observe();
      return (
        latest.turnId === turnId && SETTLED_TURN_STATES.includes(latest.state)
      );
    }, deadline);
    return latest;
  };
  for (const step of parityStepsFor(state)) {
    const scope = step.scope ?? "pane";
    const selector = step.selector ? scopeToTurn(step.selector, turnId) : null;
    if (step.op === "settle") await settled();
    else if (step.op === "submit") turnId = await submit();
    else if (step.op === "open") {
      await settled();
      await waitForState(() => evaluate(openExpression(selector)), deadline);
    } else if (step.op === "click")
      await waitForState(
        () =>
          evaluate(scopedClickExpression(selector, scope, captureChat.chatId)),
        deadline,
      );
    else if (step.op === "fill")
      await waitForState(
        () =>
          evaluate(
            fillExpression(selector, scope, step.text, captureChat.chatId),
          ),
        deadline,
      );
    else if (step.op === "select")
      await dragSelect(
        cdp,
        await located(
          evaluate,
          phraseRectExpression(turnId, step.phrase),
          deadline,
        ),
        deadline,
      );
    else if (step.op === "hover") {
      const center = await located(
        evaluate,
        centerExpression(selector, scope, captureChat.chatId),
        deadline,
      );
      const at = Math.min(deadline, Date.now() + 5000);
      // Enter from outside so pointerenter fires on the target.
      await cdp.mouse({ type: "mouseMoved", x: 0, y: 0 }, at);
      await cdp.mouse({ type: "mouseMoved", ...center }, at);
    } else if (step.op === "key") {
      await waitForState(
        () => evaluate(focusExpression(step.focus, captureChat.chatId)),
        deadline,
      );
      const at = Math.min(deadline, Date.now() + 5000);
      await cdp.key({ type: "rawKeyDown", key: step.key, ctrl: step.ctrl }, at);
      await cdp.key({ type: "keyUp", key: step.key, ctrl: step.ctrl }, at);
    } else throw new Error("parity-step");
    // One bounded step per action: probes hold at most 64 timestamps.
    record("parity-" + step.op, await observe());
  }
  return turnId;
}

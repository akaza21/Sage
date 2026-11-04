import { existsSync, lstatSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import {
  assertEvidenceRoot,
  attachCdp,
  decodeBoundedScreenshot,
  discoverTarget,
  operationDeadline,
  revalidateAttachedPage,
  writeContainedBytes,
  writeContainedText,
  writePendingJourneyReceipt,
} from "./source-functional-native-ide-journey.mjs";
import {
  finalizeAcp04Collection,
  preflightCodex,
  revalidateCollectionIdentities,
  validateRuntimeObservation,
} from "./source-functional-acp04-finalizer.mjs";
import {
  ACP04_ECHOES,
  codexSemanticVersion,
  createAcp04Receipt,
  parseAcp04ProbeArguments,
} from "./source-functional-acp04-live-contract.mjs";
import {
  createModeBCancelUi,
  runAcp04ModeBWithDriver,
} from "./source-functional-acp04-mode-b.mjs";
import {
  fail,
  rejectObservedFailures,
  validateSnapshot,
  waitUntil,
} from "./source-functional-acp04-observation.mjs";

export {
  ACP04_ECHOES,
  ACP04_MODE_A,
  ACP04_MODE_B,
  createAcp04Receipt,
  parseAcp04ProbeArguments,
} from "./source-functional-acp04-live-contract.mjs";
export { finalizeAcp04Collection } from "./source-functional-acp04-finalizer.mjs";
export {
  ACP04_MODE_B_CANCEL_PROMPT,
  ACP04_MODE_B_RECOVERY_ECHO,
  acceptModeBSnapshot,
  createModeBState,
} from "./source-functional-acp04-mode-b.mjs";
export {
  fail,
  rejectObservedFailures,
  validSelectIdentity,
  validateSnapshot,
  waitUntil,
} from "./source-functional-acp04-observation.mjs";

export const ACP04_PROJECTION_UNAVAILABLE = Object.freeze({
  kind: "projection-unavailable",
});

function projectionUnavailable(value) {
  return (
    value !== null &&
    typeof value === "object" &&
    Object.keys(value).length === 1 &&
    value.kind === ACP04_PROJECTION_UNAVAILABLE.kind
  );
}

// Turn states come from each Turn's data-turn-state hook and are named with
// the transcript's own status vocabulary, so a Turn the person cancelled
// still reads as Cancelled or Interrupted.
export const ACP04_TURN_STATE_LABELS = Object.freeze({
  queued: "Queued",
  sending: "Sending",
  running: "Running",
  waitingModel: "Waiting for model",
  waitingTool: "Waiting for tool",
  waitingApproval: "Waiting for approval",
  waitingUser: "Waiting for user",
  cancelling: "Cancelling",
  completed: "Completed",
  interrupted: "Interrupted",
  cancelled: "Cancelled",
  failed: "Failed",
  deliveryUnknown: "Delivery unknown",
});
export const ACP04_TURN_STATE_EXPRESSION = `(t=>{const labels=${JSON.stringify(ACP04_TURN_STATE_LABELS)},state=t.dataset.turnState||'';return Object.hasOwn(labels,state)?labels[state]:''})`;

// The Codex harness option's data-option-id and its rendered trigger name.
export const ACP04_CODEX_HARNESS_ID = "codex";
export const ACP04_CODEX_HARNESS_NAME = "Codex";

const FOCUSED_PANES =
  "[...document.querySelectorAll('section.agent-chat-pane.agent-chat-pane--focused')]";
const CURRENT_RAIL_CHATS =
  '[...document.querySelectorAll(\'aside[aria-label="Projects and chats"] button[aria-current="page"][data-chat-id]\')]';
const HARNESS_TRIGGERS =
  '[...s.querySelectorAll(\'button[aria-label="Harness"][aria-haspopup="listbox"]\')]';
const MODEL_TRIGGERS =
  '[...s.querySelectorAll(\'button[aria-label="Model"][aria-haspopup="listbox"]\')]';
const HARNESS_STATUS =
  "(()=>{const x=[...document.querySelectorAll('[aria-label^=\"Harness status: \"]')];return x.length===1?x[0].getAttribute('aria-label').slice(16).trim():''})()";
const AGENT_MODE =
  '(()=>{const groups=[...document.querySelectorAll(\'[role="radiogroup"][aria-label="Window mode"]\')];if(groups.length!==1)return false;const agent=[...groups[0].querySelectorAll(\'input[name="workbench-window-mode"][value="agent"]:checked\')];return agent.length===1&&document.querySelectorAll(\'main.workbench.workbench--agent\').length===1})()';
const CODEX_OPTIONS = `(harness=>{const controls=harness.getAttribute('aria-controls'),listbox=controls?document.getElementById(controls):null;if(!listbox||listbox.getAttribute('role')!=='listbox')return null;return [...listbox.querySelectorAll('[role="option"]')].filter(x=>x.dataset.optionId===${JSON.stringify(ACP04_CODEX_HARNESS_ID)}&&x.getAttribute('aria-disabled')!=='true'&&!x.disabled)})`;
const AGENT_SURFACES =
  "[...panes[0].querySelectorAll('section.chat-surface[aria-label=\"Agent chat\"]')]";

// Names the first unmet precondition of openCodexPicker, so a picker that
// never becomes openable fails with the reason rather than a bare code.
export const ACP04_CODEX_PICKER_STATES = Object.freeze([
  "ready",
  "pane-count",
  "surface-count",
  "trigger-count",
  "trigger-disabled",
  "trigger-expanded",
]);
const CODEX_PICKER_STATE_EXPRESSION = `(()=>{const panes=${FOCUSED_PANES};if(panes.length!==1)return 'pane-count';const surfaces=${AGENT_SURFACES};if(surfaces.length!==1)return 'surface-count';const s=surfaces[0],harness=${HARNESS_TRIGGERS};if(harness.length!==1)return 'trigger-count';if(harness[0].disabled)return 'trigger-disabled';if(harness[0].getAttribute('aria-expanded')!=='false')return 'trigger-expanded';return 'ready'})()`;

// Waits, within the journey deadline, for a fresh chat's Harness trigger to
// become openable. On timeout the failure code carries the last observed
// state; an observation outside the known states fails immediately.
export function awaitCodexPickerOpenable(read, deadline) {
  return awaitUiState(
    read,
    deadline,
    ACP04_CODEX_PICKER_STATES,
    "codex-picker",
  );
}

// Names the first unmet condition of Codex readiness after selection, so a
// harness that never becomes ready says which part of the surface held it.
export const ACP04_CODEX_READY_STATES = Object.freeze([
  "ready",
  "pane-count",
  "surface-count",
  "projection-count",
  "control-count",
  "harness-disabled",
  "model-disabled",
  "harness-not-codex",
  "model-unidentified",
  "harness-not-ready",
  "composer",
]);
function codexReadyStateExpression(codexId, codexName) {
  return `(()=>{const panes=${FOCUSED_PANES};if(panes.length!==1)return 'pane-count';const surfaces=${AGENT_SURFACES};if(surfaces.length!==1)return 'surface-count';const s=surfaces[0],p=s.querySelectorAll('section.acp-chat-projection[aria-label^="Agent chat "]'),harness=${HARNESS_TRIGGERS},models=${MODEL_TRIGGERS},composers=[...s.querySelectorAll('textarea[aria-label="Message agent"]')],bounded=x=>x.length>0&&x.length<=512,status=${HARNESS_STATUS};if(p.length!==1)return 'projection-count';if(harness.length!==1||models.length!==1)return 'control-count';if(harness[0].disabled)return 'harness-disabled';if(models[0].disabled)return 'model-disabled';if(harness[0].dataset.selectedId!==${codexId}||harness[0].textContent.trim()!==${codexName})return 'harness-not-codex';if(!bounded(models[0].dataset.selectedId||'')||!bounded(models[0].textContent.trim()))return 'model-unidentified';if(status!=='Ready'&&status!=='Live')return 'harness-not-ready';if(composers.length!==1||composers[0].disabled!==false)return 'composer';return 'ready'})()`;
}

export function awaitCodexReady(read, deadline) {
  return awaitUiState(read, deadline, ACP04_CODEX_READY_STATES, "codex-ready");
}

async function awaitUiState(read, deadline, states, name) {
  let last = "unobserved";
  while (Date.now() < deadline) {
    const state = await read();
    if (!states.includes(state)) fail(`${name}-state-invalid`);
    if (state === "ready") return;
    last = state;
    await new Promise((resolve) =>
      setTimeout(resolve, Math.min(100, Math.max(0, deadline - Date.now()))),
    );
  }
  fail(`${name}-${last}-timeout`);
}

export function createAcp04Ui(codexVersion) {
  if (!codexSemanticVersion(codexVersion)) fail("codex-version-invalid");
  const codexId = JSON.stringify(ACP04_CODEX_HARNESS_ID);
  const codexName = JSON.stringify(ACP04_CODEX_HARNESS_NAME);
  return Object.freeze({
    editorReady:
      '(()=>{const mains=[...document.querySelectorAll(\'main.workbench.workbench--editor\')],groups=[...document.querySelectorAll(\'[role="radiogroup"][aria-label="Window mode"]\')];if(mains.length!==1||groups.length!==1)return false;const editor=[...groups[0].querySelectorAll(\'input[name="workbench-window-mode"][value="editor"]\')],agent=[...groups[0].querySelectorAll(\'input[name="workbench-window-mode"][value="agent"]\')];return editor.length===1&&agent.length===1&&editor[0].checked&&!agent[0].checked})()',
    enterAgentFromEditor:
      '(()=>{const mains=[...document.querySelectorAll(\'main.workbench.workbench--editor\')],groups=[...document.querySelectorAll(\'[role="radiogroup"][aria-label="Window mode"]\')];if(mains.length!==1||groups.length!==1)return false;const editor=[...groups[0].querySelectorAll(\'input[name="workbench-window-mode"][value="editor"]\')];const agent=[...groups[0].querySelectorAll(\'input[name="workbench-window-mode"][value="agent"]\')];if(editor.length!==1||agent.length!==1||!editor[0].checked||agent[0].checked)return false;agent[0].click();return true})()',
    seededAgentReady: `(()=>{const allPanes=[...document.querySelectorAll('section.agent-chat-pane')],panes=${FOCUSED_PANES},rails=${CURRENT_RAIL_CHATS};if(!${AGENT_MODE}||allPanes.length!==1||panes.length!==1||rails.length!==1)return false;return ${AGENT_SURFACES}.length===1})()`,
    seededChatId: `(()=>{const panes=${FOCUSED_PANES},rails=${CURRENT_RAIL_CHATS};if(panes.length!==1||rails.length!==1)return null;return rails[0].dataset.chatId||null})()`,
    createFreshChat:
      "(()=>{const sidebars=[...document.querySelectorAll('aside[aria-label=\"Projects and chats\"]')];if(sidebars.length!==1)return false;const buttons=[...sidebars[0].querySelectorAll('button[aria-label=\"New chat\"]')].filter(x=>!x.disabled);if(buttons.length!==1)return false;buttons[0].click();return true})()",
    codexPickerState: CODEX_PICKER_STATE_EXPRESSION,
    openCodexPicker: `(()=>{const panes=${FOCUSED_PANES};if(panes.length!==1)return false;const surfaces=${AGENT_SURFACES};if(surfaces.length!==1)return false;const s=surfaces[0],harness=${HARNESS_TRIGGERS};if(harness.length!==1||harness[0].disabled||harness[0].getAttribute('aria-expanded')!=='false')return false;harness[0].click();return true})()`,
    codexSelectable: `(()=>{const allPanes=[...document.querySelectorAll('section.agent-chat-pane')],panes=${FOCUSED_PANES},rails=${CURRENT_RAIL_CHATS};if(!${AGENT_MODE}||allPanes.length<1||allPanes.length>2||panes.length!==1||rails.length!==1)return false;const surfaces=${AGENT_SURFACES};if(surfaces.length!==1)return false;const s=surfaces[0],harness=${HARNESS_TRIGGERS},models=${MODEL_TRIGGERS};if(harness.length!==1||models.length!==1||harness[0].disabled||harness[0].getAttribute('aria-expanded')!=='true')return false;const options=${CODEX_OPTIONS}(harness[0]);return options!==null&&options.length===1})()`,
    selectCodex: `(()=>{const panes=${FOCUSED_PANES};if(panes.length!==1)return false;const surfaces=${AGENT_SURFACES};if(surfaces.length!==1)return false;const s=surfaces[0],harness=${HARNESS_TRIGGERS};if(harness.length!==1||harness[0].disabled||harness[0].getAttribute('aria-expanded')!=='true')return false;const options=${CODEX_OPTIONS}(harness[0]);if(options===null||options.length!==1)return false;options[0].click();return true})()`,
    codexReadyState: codexReadyStateExpression(codexId, codexName),
    codexReady: `(${codexReadyStateExpression(codexId, codexName)}==='ready')`,
    sendEnabled: `(()=>{const panes=${FOCUSED_PANES};return panes.length===1&&panes[0].querySelectorAll('button[aria-label="Send message"]:not(:disabled)').length===1})()`,
    send: `(()=>{const panes=${FOCUSED_PANES};if(panes.length!==1)return false;const x=[...panes[0].querySelectorAll('button[aria-label="Send message"]:not(:disabled)')];if(x.length!==1)return false;x[0].click();return true})()`,
    ...createModeBCancelUi(),
  });
}

export const ACP04_SNAPSHOT_EXPRESSION = `(()=>{const allPanes=[...document.querySelectorAll('section.agent-chat-pane')],panes=${FOCUSED_PANES};if(allPanes.length<1||allPanes.length>2||panes.length!==1)return null;const surfaces=${AGENT_SURFACES};if(surfaces.length!==1)return null;const s=surfaces[0],projections=[...s.querySelectorAll('section.acp-chat-projection[aria-label^="Agent chat "]')],harness=${HARNESS_TRIGGERS},models=${MODEL_TRIGGERS},composers=[...s.querySelectorAll('textarea[aria-label="Message agent"]')];if(harness.length!==1||models.length!==1)return null;if(projections.length===0)return {kind:'projection-unavailable'};if(projections.length!==1)return null;const p=projections[0],harnessText=harness[0].textContent.trim(),modelText=models[0].textContent.trim(),modelId=models[0].dataset.selectedId||'';const alerts=[...s.querySelectorAll('[role="alert"]')].map(x=>{const source=(String(x.className).trim().split(/\\s+/u)[0]||x.tagName.toLowerCase()).replace(/[^a-z0-9_-]/giu,'-').slice(0,64)||'unclassified';return '['+source+'] '+x.innerText.trim()}).filter(x=>!x.endsWith('] '));const named=(s.innerText.match(/ProtocolFailure|HostError::Closed|ACP[^\\n]{0,80}(?:recovery|capacity)/giu)||[]).map(x=>'[named-protocol] '+x);return {projectId:p.dataset.projectId||'',chatId:p.dataset.chatId||'',currentSessionId:p.dataset.currentSessionId||'',projectionRevision:p.dataset.projectionRevision||'',harness:harness[0].dataset.selectedId&&harnessText?{id:harness[0].dataset.selectedId,text:harnessText}:null,model:modelId&&modelText?{id:modelId,text:modelText}:null,harnessStatus:${HARNESS_STATUS},composerEnabled:composers.length===1&&composers[0].disabled===false,failures:[...new Set([...alerts,...named])],turns:[...p.querySelectorAll('section[data-turn-id][data-turn-ordinal]')].map(t=>({id:t.dataset.turnId||'',sessionId:t.dataset.sessionId||'',runtimeEpoch:t.dataset.runtimeEpoch||'',ordinal:Number(t.dataset.turnOrdinal),state:${ACP04_TURN_STATE_EXPRESSION}(t),items:[...t.querySelectorAll('article[data-item-id][data-message-role]')].map(x=>({id:x.dataset.itemId||'',role:x.dataset.messageRole||'',text:(x.querySelector('.acp-markdown')?.innerText||'').trim()}))}))}})()`;

export function freshChatReadyExpression(previousChatId) {
  if (
    typeof previousChatId !== "string" ||
    previousChatId.length === 0 ||
    previousChatId.length > 512
  )
    fail("seeded-chat-id-invalid");
  return `(()=>{const allPanes=[...document.querySelectorAll('section.agent-chat-pane')],panes=${FOCUSED_PANES},rails=${CURRENT_RAIL_CHATS};if(!${AGENT_MODE}||allPanes.length<1||allPanes.length>2||panes.length!==1||rails.length!==1)return false;if(${AGENT_SURFACES}.length!==1)return false;const chatId=rails[0].dataset.chatId||'';return chatId!==${JSON.stringify(previousChatId)}&&chatId!==''})()`;
}

const RUNTIME_OBSERVATION_EXPRESSION =
  '(()=>{const invoke=window.__TAURI_INTERNALS__?.invoke;if(typeof invoke!=="function")throw new Error("Sage invoke unavailable");return invoke("get_codex_runtime_identity",{})})()';

export async function probeAcp04Snapshot(cdp, deadline) {
  const value = await cdp.evaluate(
    ACP04_SNAPSHOT_EXPRESSION,
    operationDeadline(deadline),
  );
  return projectionUnavailable(value)
    ? ACP04_PROJECTION_UNAVAILABLE
    : validateSnapshot(value);
}

function validateEchoes(echoes) {
  if (
    !Array.isArray(echoes) ||
    echoes.length !== 2 ||
    new Set(echoes).size !== 2 ||
    !echoes.every(
      (item) => typeof item === "string" && /^[A-Z0-9_]{8,80}$/u.test(item),
    )
  )
    fail("echoes-invalid");
}

export function createModeAState(
  echoes = ACP04_ECHOES,
  initialValue,
  codexVersion,
) {
  validateEchoes(echoes);
  // The picker names the harness only; the runtime observation binds the
  // admitted Codex version at the end of each mode.
  if (!codexSemanticVersion(codexVersion)) fail("codex-version-invalid");
  const initial = validateSnapshot(initialValue);
  rejectObservedFailures(initial);
  if (
    initial.turns.length !== 0 ||
    initial.currentSessionId !== "" ||
    initial.harness.id !== ACP04_CODEX_HARNESS_ID ||
    initial.harness.text !== ACP04_CODEX_HARNESS_NAME ||
    !["Ready", "Live"].includes(initial.harnessStatus) ||
    !initial.composerEnabled
  )
    fail("chat-not-clean");
  return Object.freeze({
    phase: "await-first",
    echoes: [...echoes],
    projectId: initial.projectId,
    chatId: initial.chatId,
    sessionId: null,
    projectionRevision: initial.projectionRevision,
    turnIds: [],
    turnOrdinals: [],
    itemIds: [],
    runtimeEpoch: null,
    harness: { ...initial.harness },
    model: { ...initial.model },
  });
}

export function acceptModeASnapshot(state, value) {
  const snapshot = validateSnapshot(value);
  rejectObservedFailures(snapshot);
  if (state.phase === "complete") return state;
  if (
    snapshot.projectId !== state.projectId ||
    snapshot.chatId !== state.chatId ||
    snapshot.harness.id !== state.harness.id ||
    snapshot.harness.text !== state.harness.text ||
    snapshot.model.id !== state.model.id ||
    snapshot.model.text !== state.model.text
  )
    fail("identity-churn");
  const revision = BigInt(snapshot.projectionRevision);
  const priorRevision = BigInt(state.projectionRevision);
  if (revision < priorRevision) fail("projection-stale");
  const index = state.phase === "await-first" ? 0 : 1;
  if (snapshot.turns.length < index + 1) {
    if (revision === priorRevision) return state;
    return state;
  }
  if (snapshot.turns.length !== index + 1) fail("turn-cardinality");
  for (let prior = 0; prior < index; prior += 1) {
    if (snapshot.turns[prior].id !== state.turnIds[prior]) fail("turn-churn");
  }
  const turn = snapshot.turns[index];
  if (
    ["Failed", "Cancelled", "Interrupted", "Delivery unknown"].includes(
      turn.state,
    )
  )
    fail("turn-terminal-failure");
  if (turn.state !== "Completed") return state;
  if (revision <= priorRevision) fail("projection-stale");
  const prompt = `Reply with exactly: ${state.echoes[index]}`;
  if (
    turn.items.length !== 2 ||
    turn.items[0].role !== "user" ||
    turn.items[0].text !== prompt ||
    turn.items[1].role !== "assistant" ||
    turn.items[1].text !== state.echoes[index]
  )
    fail("echo-mismatch");
  if (
    !snapshot.currentSessionId ||
    turn.sessionId !== snapshot.currentSessionId ||
    (state.sessionId && turn.sessionId !== state.sessionId) ||
    (state.runtimeEpoch && turn.runtimeEpoch !== state.runtimeEpoch) ||
    snapshot.turns.some((candidate) => candidate.sessionId !== turn.sessionId)
  )
    fail("session-churn");
  if (
    !["Ready", "Live"].includes(snapshot.harnessStatus) ||
    !snapshot.composerEnabled
  )
    fail("completion-not-ready");
  return Object.freeze({
    ...state,
    phase: index === 0 ? "await-second" : "complete",
    sessionId: turn.sessionId,
    runtimeEpoch: turn.runtimeEpoch,
    projectionRevision: snapshot.projectionRevision,
    turnIds: [...state.turnIds, turn.id],
    turnOrdinals: [...state.turnOrdinals, turn.ordinal],
    itemIds: [...state.itemIds, ...turn.items.map((item) => item.id)],
  });
}

export function fillPromptExpression(prompt) {
  return `(()=>{const panes=[...document.querySelectorAll('section.agent-chat-pane.agent-chat-pane--focused')];if(panes.length!==1)return false;const x=[...panes[0].querySelectorAll('textarea[aria-label="Message agent"]')];if(x.length!==1||x[0].disabled!==false)return false;const set=Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype,'value').set;set.call(x[0],${JSON.stringify(prompt)});x[0].dispatchEvent(new Event('input',{bubbles:true}));return true})()`;
}

function defaultDriver(cdp, deadline, codexVersion, guard = () => {}) {
  const ui = createAcp04Ui(codexVersion);
  const exactAction = async (expression, code) => {
    guard();
    if ((await cdp.evaluate(expression, operationDeadline(deadline))) !== true)
      fail(code);
  };
  const readiness = (expression) => {
    guard();
    return cdp.evaluate(expression, operationDeadline(deadline));
  };
  return {
    waitForEditorReady: () =>
      waitUntil(
        () => readiness(ui.editorReady),
        deadline,
        "acp04-editor-ready-timeout",
      ),
    enterAgentFromEditor: () =>
      waitUntil(
        () => readiness(ui.enterAgentFromEditor),
        deadline,
        "acp04-window-mode-invalid",
      ),
    waitForSeededAgentChat: () =>
      waitUntil(
        () => readiness(ui.seededAgentReady),
        deadline,
        "acp04-seeded-chat-timeout",
      ),
    seededChatId: () =>
      waitUntil(
        async () => {
          guard();
          const chatId = await cdp.evaluate(
            ui.seededChatId,
            operationDeadline(deadline),
          );
          if (chatId === null) return null;
          if (
            typeof chatId !== "string" ||
            chatId.length === 0 ||
            chatId.length > 512
          )
            fail("seeded-chat-id-invalid");
          return chatId;
        },
        deadline,
        "acp04-seeded-chat-id-timeout",
      ),
    createFreshChat: () =>
      exactAction(ui.createFreshChat, "new-chat-ambiguous"),
    waitForFreshChat: (seededChatId) =>
      waitUntil(
        () => readiness(freshChatReadyExpression(seededChatId)),
        deadline,
        "acp04-fresh-chat-timeout",
      ),
    waitForCodexSelectable: async () => {
      await awaitCodexPickerOpenable(
        () => readiness(ui.codexPickerState),
        deadline,
      );
      await exactAction(ui.openCodexPicker, "codex-picker-open-invalid");
      return waitUntil(
        () => readiness(ui.codexSelectable),
        deadline,
        "acp04-codex-selectable-timeout",
      );
    },
    selectCodex: () => exactAction(ui.selectCodex, "codex-selector-invalid"),
    waitForCodexReady: () =>
      awaitCodexReady(() => readiness(ui.codexReadyState), deadline),
    snapshot: () => {
      guard();
      return probeAcp04Snapshot(cdp, deadline);
    },
    fillPrompt: (prompt) =>
      exactAction(fillPromptExpression(prompt), "composer-invalid"),
    waitForSendReady: () =>
      waitUntil(
        () => readiness(ui.sendEnabled),
        deadline,
        "acp04-send-ready-timeout",
      ),
    send: () => exactAction(ui.send, "send-ambiguous"),
    waitForCancelReady: () =>
      waitUntil(
        () => readiness(ui.cancelEnabled),
        deadline,
        "acp04-cancel-ready-timeout",
      ),
    cancel: () => exactAction(ui.cancel, "cancel-ambiguous"),
    runtimeObservation: () => {
      guard();
      return cdp.evaluate(
        RUNTIME_OBSERVATION_EXPRESSION,
        operationDeadline(deadline),
      );
    },
  };
}

export async function runAcp04ModeA(
  cdp,
  deadline,
  echoes = ACP04_ECHOES,
  deps = {},
) {
  if (!codexSemanticVersion(deps.codexVersion)) fail("codex-version-invalid");
  const driver =
    deps.driver ?? defaultDriver(cdp, deadline, deps.codexVersion, deps.guard);
  const startedAt = new Date().toISOString();
  await driver.waitForEditorReady();
  await driver.enterAgentFromEditor();
  await driver.waitForSeededAgentChat();
  const seededChatId = await driver.seededChatId();
  await driver.createFreshChat();
  await driver.waitForFreshChat(seededChatId);
  await driver.waitForCodexSelectable();
  await driver.selectCodex();
  await driver.waitForCodexReady?.();
  const initial = await driver.snapshot();
  if (projectionUnavailable(initial)) fail("initial-projection-unavailable");
  let state = createModeAState(echoes, initial, deps.codexVersion);
  const observations = [];
  for (const echo of echoes) {
    const before = state.phase;
    await driver.fillPrompt(`Reply with exactly: ${echo}`);
    await driver.waitForSendReady();
    await driver.send();
    const accepted = await waitUntil(
      async () => {
        const snapshot = await driver.snapshot();
        if (projectionUnavailable(snapshot)) return null;
        const next = acceptModeASnapshot(state, snapshot);
        return next.phase === before ? null : { next, snapshot };
      },
      deadline,
      "acp04-turn-timeout",
    );
    state = accepted.next;
    observations.push(accepted.snapshot);
  }
  if (state.phase !== "complete") fail("mode-incomplete");
  const runtimeObservation = validateRuntimeObservation(
    await driver.runtimeObservation(),
    state,
  );
  if (runtimeObservation.admittedEntrypoint.version !== deps.codexVersion)
    fail("runtime-version-mismatch");
  return Object.freeze({
    startedAt,
    completedAt: new Date().toISOString(),
    state,
    observations,
    runtimeObservation,
  });
}

export async function runAcp04ModeB(cdp, deadline, modeAState, deps = {}) {
  const driver =
    deps.driver ?? defaultDriver(cdp, deadline, deps.codexVersion, deps.guard);
  return runAcp04ModeBWithDriver(deadline, modeAState, {
    driver,
    codexVersion: deps.codexVersion,
  });
}

async function execute(options) {
  assertEvidenceRoot(options.sourceRoot, options.output);
  const deadline = Date.now() + options.deadlineMs;
  options = await preflightCodex(options, deadline);
  const admitted = await discoverTarget(
    options.port,
    options.expectedOrigin,
    deadline,
  );
  const cdp = await attachCdp(admitted, deadline);
  let collected;
  let collectionError;
  try {
    await revalidateAttachedPage(options, cdp, admitted, deadline);
    const guard = () => {
      for (const logPath of [
        options.nativeLogs.stdout,
        options.nativeLogs.stderr,
      ]) {
        const item = existsSync(logPath) ? lstatSync(logPath) : null;
        if (
          !item ||
          item.isSymbolicLink() ||
          !item.isFile() ||
          item.size > 1024 * 1024
        )
          fail("native-log-bound");
      }
    };
    const modeA = await runAcp04ModeA(cdp, deadline, ACP04_ECHOES, {
      codexVersion: options.codex.version,
      guard,
    });
    const modeB = await runAcp04ModeB(cdp, deadline, modeA.state, {
      codexVersion: options.codex.version,
      guard,
    });
    const result = Object.freeze({
      startedAt: modeA.startedAt,
      completedAt: modeB.completedAt,
      state: modeB.state,
      modeA,
      modeB,
      runtimeObservation: modeB.runtimeObservation,
    });
    const transcriptName = "acp04-mode-b-observations.json";
    writeContainedText(
      options.sourceRoot,
      options.output,
      transcriptName,
      `${JSON.stringify(result, null, 2)}\n`,
    );
    const screenshotName = "acp04-mode-b-complete.png";
    writeContainedBytes(
      options.sourceRoot,
      options.output,
      screenshotName,
      decodeBoundedScreenshot(
        await cdp.screenshot(operationDeadline(deadline)),
      ),
    );
    const target = await revalidateAttachedPage(
      options,
      cdp,
      admitted,
      deadline,
    );
    collected = {
      result,
      target,
      artifacts: [
        path.join(options.output, transcriptName),
        path.join(options.output, screenshotName),
      ],
      stats: {
        connectionCount: cdp.stats.connectionCount,
        maxInFlight: cdp.stats.maxInFlight,
        requestCount: cdp.stats.requestCount,
      },
    };
  } catch (error) {
    collectionError = error;
  }
  if (collectionError) {
    try {
      await cdp.close(Date.now() + 5_000);
    } catch {
      /* original failure remains primary */
    }
    throw collectionError;
  }
  return finalizeAcp04Collection({
    closeCdp: () => cdp.close(Date.now() + 5_000),
    revalidateIdentities: () =>
      revalidateCollectionIdentities(options, deadline),
    createReceipt: () =>
      createAcp04Receipt(
        options,
        collected.result,
        collected.target,
        collected.artifacts,
        collected.stats,
      ),
    writePendingReceipt: (receipt) =>
      writePendingJourneyReceipt(
        options.sourceRoot,
        options.output,
        options.receiptName,
        receipt,
      ),
  });
}

if (
  process.argv[1] &&
  path.resolve(fileURLToPath(import.meta.url)) === path.resolve(process.argv[1])
) {
  let options;
  try {
    options = parseAcp04ProbeArguments(process.argv.slice(2));
    await execute(options);
  } catch (error) {
    try {
      if (options?.output && existsSync(options.output))
        writeContainedText(
          options.sourceRoot,
          options.output,
          "machine-failure.json",
          `${JSON.stringify(
            {
              schemaVersion: 1,
              completed: false,
              error: String(error?.message ?? error).slice(0, 1024),
            },
            null,
            2,
          )}\n`,
        );
    } catch {
      /* unsafe roots never receive a failure record */
    }
    console.error(String(error?.message ?? error).slice(0, 1024));
    process.exitCode = 1;
  }
}

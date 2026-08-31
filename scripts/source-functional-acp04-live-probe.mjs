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

export {
  ACP04_ECHOES,
  ACP04_MODE_A,
  createAcp04Receipt,
  parseAcp04ProbeArguments,
} from "./source-functional-acp04-live-contract.mjs";
export { finalizeAcp04Collection } from "./source-functional-acp04-finalizer.mjs";

const FAILURE =
  /ProtocolFailure|HostError::Closed|ACP[^\n]{0,80}(?:recovery|capacity)/iu;
const DECIMAL_REVISION = /^(?:0|[1-9]\d*)$/u;

function fail(code) {
  throw new Error(`acp04-${code}`);
}

export function createAcp04Ui(codexVersion) {
  if (!codexSemanticVersion(codexVersion)) fail("codex-version-invalid");
  const display = `Codex \u2014 ${codexVersion}`;
  return Object.freeze({
    editorReady:
      '(()=>{const mains=[...document.querySelectorAll(\'main.workbench.workbench--editor\')],groups=[...document.querySelectorAll(\'[role="radiogroup"][aria-label="Window mode"]\')];if(mains.length!==1||groups.length!==1)return false;const editor=[...groups[0].querySelectorAll(\'input[name="workbench-window-mode"][value="editor"]\')],agent=[...groups[0].querySelectorAll(\'input[name="workbench-window-mode"][value="agent"]\')];return editor.length===1&&agent.length===1&&editor[0].checked&&!agent[0].checked})()',
    enterAgentFromEditor:
      '(()=>{const mains=[...document.querySelectorAll(\'main.workbench.workbench--editor\')],groups=[...document.querySelectorAll(\'[role="radiogroup"][aria-label="Window mode"]\')];if(mains.length!==1||groups.length!==1)return false;const editor=[...groups[0].querySelectorAll(\'input[name="workbench-window-mode"][value="editor"]\')];const agent=[...groups[0].querySelectorAll(\'input[name="workbench-window-mode"][value="agent"]\')];if(editor.length!==1||agent.length!==1||!editor[0].checked||agent[0].checked)return false;agent[0].click();return true})()',
    seededAgentReady:
      "(()=>{const groups=[...document.querySelectorAll('[role=\"radiogroup\"][aria-label=\"Window mode\"]')];const panes=[...document.querySelectorAll('section.agent-chat-pane')];const rails=[...document.querySelectorAll('.project-chat[aria-current=\"page\"]')];if(groups.length!==1||panes.length!==1||rails.length!==1||!panes[0].classList.contains('agent-chat-pane--focused'))return false;const agent=[...groups[0].querySelectorAll('input[name=\"workbench-window-mode\"][value=\"agent\"]:checked')];return agent.length===1&&document.querySelectorAll('main.workbench.workbench--agent').length===1&&panes[0].querySelectorAll('section.chat-surface[aria-label=\"Agent chat\"]').length===1})()",
    codexSelectable: `(()=>{const groups=[...document.querySelectorAll('[role="radiogroup"][aria-label="Window mode"]')],panes=[...document.querySelectorAll('section.agent-chat-pane')],rails=[...document.querySelectorAll('.project-chat[aria-current="page"]')];if(groups.length!==1||panes.length!==1||rails.length!==1||!panes[0].classList.contains('agent-chat-pane--focused'))return false;const agent=[...groups[0].querySelectorAll('input[name="workbench-window-mode"][value="agent"]:checked')],surfaces=[...panes[0].querySelectorAll('section.chat-surface[aria-label="Agent chat"]')];if(agent.length!==1||document.querySelectorAll('main.workbench.workbench--agent').length!==1||surfaces.length!==1)return false;const selects=[...surfaces[0].querySelectorAll('.composer__controls select')],options=selects.length===2?[...selects[0].options].filter(x=>x.textContent.trim()===${JSON.stringify(display)}&&!x.disabled):[];return selects.length===2&&options.length===1})()`,
    selectCodex: `(()=>{const panes=[...document.querySelectorAll('section.agent-chat-pane.agent-chat-pane--focused')];if(panes.length!==1)return false;const selects=[...panes[0].querySelectorAll('.composer__controls select')];if(selects.length!==2)return false;const options=[...selects[0].options].filter(x=>x.textContent.trim()===${JSON.stringify(display)});if(options.length!==1||options[0].disabled)return false;const set=Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype,'value').set;set.call(selects[0],options[0].value);selects[0].dispatchEvent(new Event('change',{bubbles:true}));return true})()`,
    codexReady: `(()=>{const panes=[...document.querySelectorAll('section.agent-chat-pane.agent-chat-pane--focused')];if(panes.length!==1)return false;const s=panes[0].querySelector('section.chat-surface[aria-label="Agent chat"]');if(!s)return false;const p=s.querySelectorAll('section.acp-chat-projection[aria-label^="Agent chat "]');const selects=s.querySelectorAll('.composer__controls select');const status=s.querySelector('.session-state')?.textContent.trim();return p.length===1&&selects.length===2&&selects[0].selectedOptions.length===1&&selects[0].selectedOptions[0].textContent.trim()===${JSON.stringify(display)}&&selects[1].value!==''&&(status==='Ready'||status==='Live')&&s.querySelectorAll('textarea#prompt:not(:disabled)').length===1})()`,
    sendEnabled:
      "(()=>{const panes=[...document.querySelectorAll('section.agent-chat-pane.agent-chat-pane--focused')];return panes.length===1&&panes[0].querySelectorAll('button[aria-label=\"Send message\"]:not(:disabled)').length===1})()",
    send: "(()=>{const panes=[...document.querySelectorAll('section.agent-chat-pane.agent-chat-pane--focused')];if(panes.length!==1)return false;const x=[...panes[0].querySelectorAll('button[aria-label=\"Send message\"]:not(:disabled)')];if(x.length!==1)return false;x[0].click();return true})()",
  });
}

export const ACP04_SNAPSHOT_EXPRESSION = `(()=>{const panes=[...document.querySelectorAll('section.agent-chat-pane')];if(panes.length!==1||!panes[0].classList.contains('agent-chat-pane--focused'))return null;const surfaces=[...panes[0].querySelectorAll('section.chat-surface[aria-label="Agent chat"]')];if(surfaces.length!==1)return null;const s=surfaces[0],projections=[...s.querySelectorAll('section.acp-chat-projection[aria-label^="Agent chat "]')],selects=[...s.querySelectorAll('.composer__controls select')];if(projections.length!==1||selects.length!==2)return null;const p=projections[0],selected=x=>x.selectedOptions.length===1?{id:x.value,text:x.selectedOptions[0].textContent.trim()}:null;const alerts=[...s.querySelectorAll('[role="alert"]')].map(x=>x.innerText.trim()).filter(Boolean);const named=s.innerText.match(/ProtocolFailure|HostError::Closed|ACP[^\\n]{0,80}(?:recovery|capacity)/giu)||[];return {projectId:p.dataset.projectId||'',chatId:p.dataset.chatId||'',currentSessionId:p.dataset.currentSessionId||'',projectionRevision:p.dataset.projectionRevision||'',harness:selected(selects[0]),model:selected(selects[1]),harnessStatus:(s.querySelector('.session-state')?.textContent||'').trim(),composerEnabled:s.querySelectorAll('textarea#prompt:not(:disabled)').length===1,failures:[...new Set([...alerts,...named])],turns:[...p.querySelectorAll(':scope > section.acp-turn')].map(t=>({id:t.dataset.turnId||'',sessionId:t.dataset.sessionId||'',runtimeEpoch:t.dataset.runtimeEpoch||'',ordinal:Number(t.dataset.turnOrdinal),state:t.querySelector(':scope > header [data-turn-status][role="status"]')?.getAttribute('aria-label')||'',items:[...t.querySelectorAll('article[data-item-id][data-message-role]')].map(x=>({id:x.dataset.itemId||'',role:x.dataset.messageRole||'',text:(x.querySelector('.acp-markdown')?.innerText||'').trim()}))}))}})()`;

const RUNTIME_OBSERVATION_EXPRESSION =
  '(()=>{const invoke=window.__TAURI_INTERNALS__?.invoke;if(typeof invoke!=="function")throw new Error("Sage invoke unavailable");return invoke("get_codex_runtime_identity",{})})()';

function validSelectIdentity(value) {
  return (
    value &&
    typeof value.id === "string" &&
    value.id.length > 0 &&
    typeof value.text === "string" &&
    value.text.length > 0
  );
}

function validateSnapshot(value) {
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
  if (
    !value ||
    typeof value !== "object" ||
    typeof value.projectId !== "string" ||
    value.projectId.length === 0 ||
    typeof value.chatId !== "string" ||
    value.chatId.length === 0 ||
    typeof value.currentSessionId !== "string" ||
    !DECIMAL_REVISION.test(value.projectionRevision ?? "") ||
    !validSelectIdentity(value.harness) ||
    !validSelectIdentity(value.model) ||
    typeof value.harnessStatus !== "string" ||
    typeof value.composerEnabled !== "boolean" ||
    !Array.isArray(value.failures) ||
    !value.failures.every((item) => typeof item === "string") ||
    !Array.isArray(value.turns) ||
    !value.turns.every(validTurn)
  )
    fail("snapshot-invalid");
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
    fail("snapshot-invalid");
  return value;
}

function rejectObservedFailures(snapshot) {
  if (
    snapshot.failures.length > 0 ||
    snapshot.failures.some((item) => FAILURE.test(item))
  )
    fail("observed-failure");
}

export async function probeAcp04Snapshot(cdp, deadline) {
  return validateSnapshot(
    await cdp.evaluate(ACP04_SNAPSHOT_EXPRESSION, operationDeadline(deadline)),
  );
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
  const initial = validateSnapshot(initialValue);
  rejectObservedFailures(initial);
  if (
    initial.turns.length !== 0 ||
    initial.currentSessionId !== "" ||
    initial.harness.text !== `Codex \u2014 ${codexVersion}` ||
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

function fillPromptExpression(prompt) {
  return `(()=>{const panes=[...document.querySelectorAll('section.agent-chat-pane.agent-chat-pane--focused')];if(panes.length!==1)return false;const x=[...panes[0].querySelectorAll('textarea#prompt:not(:disabled)')];if(x.length!==1)return false;const set=Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype,'value').set;set.call(x[0],${JSON.stringify(prompt)});x[0].dispatchEvent(new Event('input',{bubbles:true}));return true})()`;
}

async function waitUntil(action, deadline, message) {
  while (Date.now() < deadline) {
    const result = await action();
    if (result) return result;
    await new Promise((resolve) =>
      setTimeout(resolve, Math.min(100, Math.max(0, deadline - Date.now()))),
    );
  }
  throw new Error(message);
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
      exactAction(ui.enterAgentFromEditor, "window-mode-invalid"),
    waitForSeededAgentChat: () =>
      waitUntil(
        () => readiness(ui.seededAgentReady),
        deadline,
        "acp04-seeded-chat-timeout",
      ),
    waitForCodexSelectable: () =>
      waitUntil(
        () => readiness(ui.codexSelectable),
        deadline,
        "acp04-codex-selectable-timeout",
      ),
    selectCodex: () => exactAction(ui.selectCodex, "codex-selector-invalid"),
    waitForCodexReady: () =>
      waitUntil(
        () => readiness(ui.codexReady),
        deadline,
        "acp04-codex-ready-timeout",
      ),
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
  await driver.waitForCodexSelectable();
  await driver.selectCodex();
  await driver.waitForCodexReady?.();
  const initial = await driver.snapshot();
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
    const result = await runAcp04ModeA(cdp, deadline, ACP04_ECHOES, {
      codexVersion: options.codex.version,
      guard: () => {
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
      },
    });
    const transcriptName = "acp04-mode-a-observations.json";
    writeContainedText(
      options.sourceRoot,
      options.output,
      transcriptName,
      `${JSON.stringify(result, null, 2)}\n`,
    );
    const screenshotName = "acp04-mode-a-complete.png";
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

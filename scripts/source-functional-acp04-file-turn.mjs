import { createHash } from "node:crypto";

import { operationDeadline } from "./source-functional-native-ide-journey.mjs";
import {
  ACP04_TURN_STATE_EXPRESSION,
  fillPromptExpression,
} from "./source-functional-acp04-live-probe.mjs";
import {
  fail,
  rejectObservedFailures,
  waitUntil,
} from "./source-functional-acp04-observation.mjs";

export const ACP04_READ_ONLY_PACKAGE_JSON_PROMPT =
  "Use exec_command exactly once with cmd `cat package.json`, shell `C:\\Program Files\\Git\\bin\\bash.exe`, and login `false`. Read package.json only. Do not modify files or run unrelated commands.";

// The transcript renders the prompt as Markdown, so its inline code spans
// lose their backticks.
export const ACP04_READ_ONLY_PACKAGE_JSON_RENDERED_PROMPT =
  ACP04_READ_ONLY_PACKAGE_JSON_PROMPT.replace(/`/gu, "");

const MAX_TURNS = 4;
const MAX_ACTIONS = 16;
const MAX_ID_BYTES = 512;
const MAX_DECIMAL_DIGITS = 20;
// Codex reports `cat package.json` as a read; a shell command card carries
// its resources the same way.
const FILE_ACTION_CARDS =
  '[role="group"][data-acp-item-id][data-activity-kind="read"],[role="group"][data-acp-item-id][data-activity-kind="execute"]';

export const ACP04_FILE_TURN_SNAPSHOT = `(()=>{const panes=document.querySelectorAll('section.agent-chat-pane.agent-chat-pane--focused');if(panes.length!==1)return null;const s=panes[0].querySelector('section.chat-surface[aria-label="Agent chat"]'),p=s?.querySelector('section.acp-chat-projection[aria-label^="Agent chat "]');if(!s||!p)return null;const turnNodes=p.querySelectorAll('section[data-turn-id][data-turn-ordinal]'),alerts=s.querySelectorAll('[role="alert"]');if(turnNodes.length>${MAX_TURNS}||alerts.length>8)return null;const boundedExact=(node,expected)=>{if(!node)return false;const walker=document.createTreeWalker(node,NodeFilter.SHOW_TEXT);let text='',next;while((next=walker.nextNode())){const part=next.nodeValue||'';if(part.length>expected.length-text.length)return false;text+=part;}return text===expected;};const read=t=>{const cards=t.querySelectorAll('${FILE_ACTION_CARDS}'),itemNodes=t.querySelectorAll('article[data-item-id][data-message-role]');if(cards.length>${MAX_ACTIONS}||itemNodes.length>16)return null;const actions=[];for(const c of cards){const buttons=c.querySelectorAll('button[data-resource-root-id][data-resource-relative-path]');if(buttons.length>${MAX_ACTIONS}||actions.length+buttons.length>${MAX_ACTIONS})return null;for(const b of buttons)actions.push({itemId:c.dataset.acpItemId||'',rootId:b.dataset.resourceRootId||'',relativePath:b.dataset.resourceRelativePath||'',enabled:!b.disabled});}const items=[];for(const x of itemNodes)items.push({id:x.dataset.itemId||'',role:x.dataset.messageRole||'',readOnlyPrompt:x.dataset.messageRole==='user'&&(boundedExact(x.querySelector('.acp-markdown'),${JSON.stringify(ACP04_READ_ONLY_PACKAGE_JSON_PROMPT)})||boundedExact(x.querySelector('.acp-markdown'),${JSON.stringify(ACP04_READ_ONLY_PACKAGE_JSON_RENDERED_PROMPT)}))});return {id:t.dataset.turnId||'',sessionId:t.dataset.sessionId||'',runtimeEpoch:t.dataset.runtimeEpoch||'',ordinal:t.dataset.turnOrdinal||'',state:${ACP04_TURN_STATE_EXPRESSION}(t),items,actions};};const turns=[];for(const node of turnNodes){const turn=read(node);if(!turn)return null;turns.push(turn);}return {projectId:p.dataset.projectId||'',chatId:p.dataset.chatId||'',currentSessionId:p.dataset.currentSessionId||'',expectedRootId:p.dataset.expectedResourceRootId||'',projectionRevision:p.dataset.projectionRevision||'',composerEnabled:s.querySelectorAll('textarea[aria-label="Message agent"]:not(:disabled)').length===1,failures:alerts.length===0?[]:['dom-alert'],turns};})()`;

export const ACP04_FILE_TAB_SNAPSHOT = `(()=>{const panes=document.querySelectorAll('aside[aria-label="Agent artifacts"]');if(panes.length!==1)return null;const pane=panes[0],tabNodes=pane.querySelectorAll('[role="tab"][data-resource-root-id][data-resource-relative-path]'),surfaces=pane.querySelectorAll('section[aria-label="File editor package.json"]'),editors=pane.querySelectorAll('section[aria-label="File editor package.json"] [aria-label="Edit package.json"]:not(:disabled)');if(tabNodes.length>${MAX_ACTIONS})return null;const tabs=[];for(const t of tabNodes)tabs.push({rootId:t.dataset.resourceRootId||'',relativePath:t.dataset.resourceRelativePath||'',selected:t.getAttribute('aria-selected')==='true'});return {projectId:pane.dataset.ownerProjectId||'',chatId:pane.dataset.ownerChatId||'',sessionId:pane.dataset.ownerSessionId||'',runtimeEpoch:pane.dataset.ownerRuntimeEpoch||'',tabs,ready:surfaces.length===1&&editors.length===1};})()`;

function validText(value, maximum = MAX_ID_BYTES) {
  return (
    typeof value === "string" &&
    value.length > 0 &&
    Buffer.byteLength(value, "utf8") <= maximum
  );
}

function invalid(value) {
  return !value || typeof value !== "object" || Array.isArray(value);
}

function validDecimalIdentity(value) {
  return (
    typeof value === "string" &&
    /^(?:0|[1-9]\d*)$/u.test(value) &&
    value.length <= MAX_DECIMAL_DIGITS &&
    BigInt(value) <= 18_446_744_073_709_551_615n
  );
}

function validateSnapshot(value) {
  if (
    invalid(value) ||
    !validText(value.projectId) ||
    !validText(value.chatId) ||
    !validText(value.currentSessionId) ||
    !validText(value.expectedRootId) ||
    !validDecimalIdentity(value.projectionRevision) ||
    typeof value.composerEnabled !== "boolean" ||
    !Array.isArray(value.failures) ||
    value.failures.length > 8 ||
    !value.failures.every((item) => item === "dom-alert") ||
    !Array.isArray(value.turns) ||
    value.turns.length > MAX_TURNS
  )
    fail("file-turn-snapshot-invalid");
  for (const turn of value.turns) {
    if (
      invalid(turn) ||
      !validText(turn.id) ||
      !validText(turn.sessionId) ||
      !validDecimalIdentity(turn.runtimeEpoch) ||
      !validDecimalIdentity(turn.ordinal) ||
      !validText(turn.state, 128) ||
      !Array.isArray(turn.items) ||
      turn.items.length > 16 ||
      !Array.isArray(turn.actions) ||
      turn.actions.length > MAX_ACTIONS
    )
      fail("file-turn-snapshot-invalid");
    for (const item of turn.items)
      if (
        invalid(item) ||
        !validText(item.id) ||
        !["user", "assistant", "system"].includes(item.role) ||
        typeof item.readOnlyPrompt !== "boolean"
      )
        fail("file-turn-snapshot-invalid");
    for (const action of turn.actions)
      if (
        invalid(action) ||
        !validText(action.itemId) ||
        !validText(action.rootId) ||
        !validText(action.relativePath) ||
        typeof action.enabled !== "boolean"
      )
        fail("file-turn-snapshot-invalid");
  }
  rejectObservedFailures(value);
  return value;
}

function modeAOwner(modeA) {
  if (
    !modeA ||
    modeA.phase !== "complete" ||
    !validText(modeA.projectId) ||
    !validText(modeA.chatId) ||
    !validText(modeA.sessionId) ||
    !validDecimalIdentity(modeA.runtimeEpoch) ||
    !Array.isArray(modeA.turnIds) ||
    modeA.turnIds.length !== 2 ||
    new Set(modeA.turnIds).size !== 2 ||
    !modeA.turnIds.every((turnId) => validText(turnId))
  )
    fail("file-turn-mode-a-invalid");
  return modeA;
}

function stablePrefix(snapshot, modeA) {
  if (
    snapshot.projectId !== modeA.projectId ||
    snapshot.chatId !== modeA.chatId ||
    snapshot.currentSessionId !== modeA.sessionId ||
    snapshot.turns.length < 2 ||
    snapshot.turns[0].id !== modeA.turnIds[0] ||
    snapshot.turns[1].id !== modeA.turnIds[1] ||
    snapshot.turns.some(
      (turn) =>
        turn.sessionId !== modeA.sessionId ||
        turn.runtimeEpoch !== modeA.runtimeEpoch,
    )
  )
    fail("file-turn-owner-churn");
}

function thirdTurn(snapshot, modeA) {
  stablePrefix(snapshot, modeA);
  if (snapshot.turns.length === 2) return null;
  if (snapshot.turns.length !== 3) fail("file-turn-cardinality");
  const turn = snapshot.turns[2];
  if (
    turn.id === modeA.turnIds[0] ||
    turn.id === modeA.turnIds[1] ||
    BigInt(turn.ordinal) <= BigInt(snapshot.turns[1].ordinal)
  )
    fail("file-turn-identity-churn");
  if (
    ["Failed", "Cancelled", "Interrupted", "Delivery unknown"].includes(
      turn.state,
    )
  )
    fail("file-turn-terminal-failure");
  if (
    turn.state !== "Completed" ||
    !snapshot.composerEnabled ||
    turn.items.length < 1 ||
    turn.items[0].role !== "user" ||
    !turn.items[0].readOnlyPrompt
  )
    return null;
  // A finished Turn folds its work behind "Worked for …"; the caller reveals it.
  if (turn.actions.length === 0) return { turn, action: null };
  const actions = turn.actions.filter((action) => action.enabled);
  if (actions.length !== 1 || turn.actions.length !== 1)
    fail("file-turn-action-ambiguous");
  return { turn, action: actions[0] };
}

function selectActionExpression(action) {
  return `(()=>{const turns=[...document.querySelectorAll('section[data-turn-id][data-turn-ordinal]')].filter(t=>t.dataset.turnId===${JSON.stringify(action.turnId)});if(turns.length!==1)return false;const actions=[...turns[0].querySelectorAll('button[data-resource-root-id][data-resource-relative-path]')].filter(x=>x.dataset.resourceRootId===${JSON.stringify(action.rootId)}&&x.dataset.resourceRelativePath===${JSON.stringify(action.relativePath)}&&x.closest(${JSON.stringify(FILE_ACTION_CARDS)})?.dataset.acpItemId===${JSON.stringify(action.itemId)}&&!x.disabled);if(actions.length!==1)return false;actions[0].click();return true;})()`;
}

// One disclosure inside the Turn: its folded "Worked for …" summary, or the
// tool rollup that summary reveals. Opens it only while it is closed and
// reports "open", "opening", "missing" or "ambiguous".
export function revealTurnDisclosureExpression(turnId, level) {
  if (!validText(turnId)) fail("file-turn-reveal-invalid");
  const candidates =
    level === "worked"
      ? "[...turns[0].querySelectorAll('details')].filter(d=>/^Worked\\b/u.test(d.querySelector(':scope > summary')?.textContent.trim()||''))"
      : level === "tool"
        ? "[...turns[0].querySelectorAll('details[data-activity-kind]')]"
        : fail("file-turn-reveal-invalid");
  return `(()=>{const turns=[...document.querySelectorAll('section[data-turn-id][data-turn-ordinal]')].filter(t=>t.dataset.turnId===${JSON.stringify(turnId)});if(turns.length!==1)return 'ambiguous';const found=${candidates};if(found.length>1)return 'ambiguous';if(found.length===0)return 'missing';if(found[0].open)return 'open';const summaries=found[0].querySelectorAll(':scope > summary');if(summaries.length!==1)return 'ambiguous';summaries[0].click();return 'opening';})()`;
}

async function revealTurnDisclosure(cdp, deadline, turnId, level) {
  const expression = revealTurnDisclosureExpression(turnId, level);
  await waitUntil(
    async () => {
      const result = await cdp.evaluate(
        expression,
        operationDeadline(deadline),
      );
      if (result === "open") return true;
      if (result === "opening" || result === "missing") return null;
      fail("file-turn-reveal-ambiguous");
    },
    deadline,
    "file-turn-reveal-timeout",
  );
}

function validateExpectedResource(value, owner) {
  if (
    invalid(value) ||
    Object.keys(value).sort().join("\0") !==
      [
        "chatId",
        "projectId",
        "relativePath",
        "rootId",
        "runtimeEpoch",
        "sessionId",
      ]
        .sort()
        .join("\0") ||
    value.projectId !== owner.projectId ||
    value.chatId !== owner.chatId ||
    value.sessionId !== owner.sessionId ||
    value.runtimeEpoch !== owner.runtimeEpoch ||
    !validText(value.rootId) ||
    value.relativePath !== "package.json"
  )
    fail("file-turn-expected-resource-invalid");
  return value;
}

function expectedResourceFromSnapshot(snapshot, owner) {
  return validateExpectedResource(
    {
      projectId: snapshot.projectId,
      chatId: snapshot.chatId,
      sessionId: snapshot.currentSessionId,
      runtimeEpoch: owner.runtimeEpoch,
      rootId: snapshot.expectedRootId,
      relativePath: "package.json",
    },
    owner,
  );
}

function validateTab(value, expected) {
  if (
    !invalid(value) &&
    (value.projectId !== expected.projectId ||
      value.chatId !== expected.chatId ||
      value.sessionId !== expected.sessionId ||
      value.runtimeEpoch !== expected.runtimeEpoch)
  )
    fail("file-turn-owner-churn");
  if (
    invalid(value) ||
    typeof value.ready !== "boolean" ||
    !Array.isArray(value.tabs) ||
    value.tabs.length > MAX_ACTIONS ||
    !value.tabs.every(
      (tab) =>
        !invalid(tab) &&
        validText(tab.rootId) &&
        validText(tab.relativePath) &&
        typeof tab.selected === "boolean",
    )
  )
    fail("file-turn-tab-invalid");
  const selected = value.tabs.filter(
    (tab) =>
      tab.selected &&
      tab.rootId === expected.rootId &&
      tab.relativePath === expected.relativePath,
  );
  return value.ready && selected.length === 1 ? selected[0] : null;
}

export function validateReadOnlyFileProof(value) {
  if (
    invalid(value) ||
    Object.keys(value).sort().join("\0") !==
      [
        "actionEnabled",
        "chatId",
        "itemId",
        "observedAt",
        "projectId",
        "relativePath",
        "rootId",
        "runtimeEpoch",
        "schemaVersion",
        "selectedReadyFile",
        "sessionId",
        "turnId",
      ]
        .sort()
        .join("\0") ||
    value.schemaVersion !== 1 ||
    !validText(value.turnId) ||
    !validText(value.itemId) ||
    !validText(value.projectId) ||
    !validText(value.chatId) ||
    !validText(value.sessionId) ||
    !validDecimalIdentity(value.runtimeEpoch) ||
    !validText(value.rootId) ||
    value.relativePath !== "package.json" ||
    value.actionEnabled !== true ||
    value.selectedReadyFile !== true ||
    !/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/u.test(
      value.observedAt ?? "",
    )
  )
    fail("file-turn-proof-invalid");
  return Object.freeze({ ...value });
}

export function createReadOnlyFileReceiptProof(proof) {
  const value = validateReadOnlyFileProof(proof);
  const hash = (item) => createHash("sha256").update(item).digest("hex");
  return Object.freeze({
    schemaVersion: 1,
    turnIdSha256: hash(value.turnId),
    itemIdSha256: hash(value.itemId),
    owner: Object.freeze({
      projectIdSha256: hash(value.projectId),
      chatIdSha256: hash(value.chatId),
      sessionIdSha256: hash(value.sessionId),
      runtimeEpoch: value.runtimeEpoch,
    }),
    resource: Object.freeze({
      rootIdSha256: hash(value.rootId),
      relativePath: value.relativePath,
    }),
    actionEnabled: true,
    selectedReadyFile: true,
    observedAt: value.observedAt,
  });
}

export async function runReadOnlyPackageJsonTurn(cdp, deadline, modeA) {
  const owner = modeAOwner(modeA);
  const snapshot = async () =>
    validateSnapshot(
      await cdp.evaluate(ACP04_FILE_TURN_SNAPSHOT, operationDeadline(deadline)),
    );
  const initial = await snapshot();
  stablePrefix(initial, owner);
  const expected = expectedResourceFromSnapshot(initial, owner);
  if (
    (await cdp.evaluate(
      fillPromptExpression(ACP04_READ_ONLY_PACKAGE_JSON_PROMPT),
      operationDeadline(deadline),
    )) !== true
  )
    fail("file-turn-prompt-ambiguous");
  const sent = await cdp.evaluate(
    "(()=>{const x=[...document.querySelectorAll('section.agent-chat-pane.agent-chat-pane--focused button[aria-label=\"Send message\"]:not(:disabled)')];if(x.length!==1)return false;x[0].click();return true})()",
    operationDeadline(deadline),
  );
  if (sent !== true) fail("file-turn-send-ambiguous");
  let accepted = await waitUntil(
    async () => thirdTurn(await snapshot(), owner),
    deadline,
    "file-turn-timeout",
  );
  if (accepted.action === null) {
    const turnId = accepted.turn.id;
    await revealTurnDisclosure(cdp, deadline, turnId, "worked");
    await revealTurnDisclosure(cdp, deadline, turnId, "tool");
    accepted = await waitUntil(
      async () => {
        const next = thirdTurn(await snapshot(), owner);
        if (next && next.turn.id !== turnId) fail("file-turn-identity-churn");
        return next?.action ? next : null;
      },
      deadline,
      "file-turn-timeout",
    );
  }
  if (
    accepted.action.rootId !== expected.rootId ||
    accepted.action.relativePath !== expected.relativePath
  )
    fail("file-turn-resource-mismatch");
  const beforeClickSnapshot = await snapshot();
  if (beforeClickSnapshot.expectedRootId !== expected.rootId)
    fail("file-turn-expected-resource-churn");
  const beforeClick = thirdTurn(beforeClickSnapshot, owner);
  if (
    !beforeClick?.action ||
    beforeClick.turn.id !== accepted.turn.id ||
    beforeClick.action.itemId !== accepted.action.itemId ||
    beforeClick.action.rootId !== expected.rootId ||
    beforeClick.action.relativePath !== expected.relativePath
  )
    fail("file-turn-before-click-churn");
  const action = { ...beforeClick.action, turnId: beforeClick.turn.id };
  if (
    (await cdp.evaluate(
      selectActionExpression(action),
      operationDeadline(deadline),
    )) !== true
  )
    fail("file-turn-click-ambiguous");
  await waitUntil(
    async () =>
      validateTab(
        await cdp.evaluate(
          ACP04_FILE_TAB_SNAPSHOT,
          operationDeadline(deadline),
        ),
        expected,
      ),
    deadline,
    "file-turn-ready-timeout",
  );
  return validateReadOnlyFileProof({
    schemaVersion: 1,
    turnId: accepted.turn.id,
    itemId: action.itemId,
    projectId: owner.projectId,
    chatId: owner.chatId,
    sessionId: owner.sessionId,
    runtimeEpoch: owner.runtimeEpoch,
    rootId: expected.rootId,
    relativePath: expected.relativePath,
    actionEnabled: true,
    selectedReadyFile: true,
    observedAt: new Date().toISOString(),
  });
}

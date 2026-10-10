import { IDENTIFIER } from "./capture-native-chat-shared.mjs";
import {
  paneSelector,
  ownedPaneNodes,
  SINGLE_CAPTURE_PANE,
} from "./capture-native-chat-pane.mjs";
export const IDLE_CAPTURE_STATES = Object.freeze([
  "new-chat",
  "composer-idle",
  "harness-menu",
  "add-menu",
]);
export const HARNESS = Object.freeze({
  pi: { id: "pi", binding: "pi-rpc", protocol: "pi-rpc-0.83.0" },
  codex: {
    id: "codex",
    binding: "codex-app-server",
    protocol: "codex-app-server-0.146.0",
  },
  claude: {
    id: "claude-code",
    binding: "claude-agent-acp",
    protocol: "standard-acp-1",
  },
});
export function visibleRectangle(rectangle, viewport) {
  return (
    [
      rectangle.x,
      rectangle.y,
      rectangle.width,
      rectangle.height,
      viewport.width,
      viewport.height,
    ].every(Number.isFinite) &&
    rectangle.width > 0 &&
    rectangle.height > 0 &&
    rectangle.x < viewport.width &&
    rectangle.y < viewport.height &&
    rectangle.x + rectangle.width > 0 &&
    rectangle.y + rectangle.height > 0
  );
}
/**
 * A closed <details> hides its descendants except its own summary. The element
 * itself may be a closed <details>: its summary row is still rendered.
 */
export function hiddenByClosedDetails(element) {
  for (
    let details = element.parentElement?.closest("details:not([open])");
    details;
    details = details.parentElement?.closest("details:not([open])")
  )
    if (!details.querySelector(":scope > summary")?.contains(element))
      return true;
  return false;
}
export function assistantTextRendered(turnId, document, visible) {
  return Array.from(document.querySelectorAll(".acp-turn"))
    .filter((element) => element.dataset.turnId === turnId)
    .some((element) =>
      Array.from(
        element.querySelectorAll(
          ".acp-message--assistant .acp-markdown, .acp-permission:not(.acp-permission--settled) > p:first-of-type",
        ),
      ).some((text) => visible(text) && text.textContent.trim().length > 0),
    );
}
/** Selects the title-bar Agent window mode; true only once it is checked. */
export const AGENT_MODE = String.raw`(()=>{const inputs=document.querySelectorAll('input[name="workbench-window-mode"][value="agent"]');if(inputs.length!==1 || inputs[0].disabled)return false;if(!inputs[0].checked)inputs[0].click();return inputs[0].checked})()`;
/**
 * Observes the newest Turn owned by the expected binding. The project welcome
 * Turn belongs to another binding and may transiently sort last.
 */
export function observationFor(binding) {
  if (!/^[a-z0-9-]{1,64}$/u.test(binding)) throw new Error("binding-identity");
  return String.raw`(async()=>{
  const expectedBinding=${JSON.stringify(binding)};
  const {useWorkbenchStore}=await import('/src/app/store.ts');
  const {acpProjectionStoreFor}=await import('/src/app/acpProjectionRegistry.ts');
  const {pinnedAgentChatOwner}=await import('/src/app/agentChatOwner.ts');
  const store=useWorkbenchStore.getState(), ownerId=pinnedAgentChatOwner() ?? store.activeTabId;
  const page=acpProjectionStoreFor(ownerId).getState().page;
  const chatId=store.tabs.find(tab=>tab.id===ownerId && tab.kind==='chat')?.id ?? null, projectId=store.project?.id ?? null;
  const paneSelector=${paneSelector.toString()};
  const ownedPaneNodes=${ownedPaneNodes.toString()};
  const queryAll=selector=>ownedPaneNodes(document,selector,chatId);
  const query=selector=>queryAll(selector)[0];
  const projectionLoaded=Boolean(page && page.chatId===chatId && page.projectId===projectId);
  const singlePane=${SINGLE_CAPTURE_PANE};
  const turns=page?.turns ?? [];
  if(turns.length>64)throw Error('projection-bound');
  const selected=query('button[aria-label="Harness"]')?.getAttribute('data-selected-id');
  const harness=store.harnesses?.find(harness=>harness.id===selected);
  const sessionOf=turn=>page?.sessions?.find(session=>session.id===turn?.sessionId);
  const owned=turns.filter(turn=>sessionOf(turn)?.bindingId===expectedBinding);
  const turn=owned.at(-1) ?? turns.at(-1), session=sessionOf(turn);
  const visibleRectangle=${visibleRectangle.toString()};
  const hiddenByClosedDetails=${hiddenByClosedDetails.toString()};
  const visible=element=>Boolean(element && element.getClientRects().length && visibleRectangle(element.getBoundingClientRect(),{width:innerWidth,height:innerHeight}) && getComputedStyle(element).visibility !== "hidden" && !hiddenByClosedDetails(element));
  const has=selector=>queryAll(selector).some(visible);
  const selectors={thought:'.acp-thought',tool:'.acp-tool-card',rollup:'.acp-tool-rollup',files:'.acp-file-changes',hunk:'.acp-inline-diff',diffTab:'[role="tab"][data-resource-kind="diff"]',subagent:'.acp-subagent-row, [data-activity-kind="subagent"]',popover:'.subagent-surface',child:'.subagent-surface__transcript .acp-message',plan:'.acp-plan',compaction:'.acp-compaction-notice',notice:'.acp-unknown',permission:'.acp-permission',pendingApproval:'.acp-permission__actions',error:'.acp-error',answer:'.acp-turn:last-of-type .acp-markdown',table:'.acp-markdown table',code:'.acp-code-block',mermaid:'.acp-mermaid',image:'.acp-image',worked:'.chat-transcript-work[open]',expanded:'.acp-tool-rollup__items',jump:'.chat-follow-end',modelMenu:'.composer__model-menu',harnessMenu:'.composer__harness-menu',effortMenu:'.composer__effort-menu',addMenu:'.composer__options',access:'[aria-label="Permission mode"]',accessMenu:'[role="listbox"][aria-label="Permission mode"]',planEntry:'[role="listbox"][aria-label="Session mode"] [data-option-id="plan"]',meter:'.composer__context [role="progressbar"]'};
  const elements=[]; let charged=0;
  for(const [kind,selector] of Object.entries(selectors))for(const element of queryAll(selector)){
    if(elements.length>=64)throw Error('probe-elements');
    const raw=(element.innerText ?? '').trim(); charged+=raw.length;
    if(raw.length>16384 || charged>65536)throw Error('probe-text-bound');
    if(/sk-(?:ant-)?|secret-command|C:\\Users\\|\/home\/|\/Users\//i.test(raw))throw Error('privacy');
    const rectangle=element.getBoundingClientRect();
    if(!visible(element))continue;
    elements.push({kind,text:raw.slice(0,128),x:rectangle.x,y:rectangle.y,width:rectangle.width,height:rectangle.height});
  }
  const controls={harness:has('button[aria-label="Harness"]'),model:has('button[aria-label="Model"]'),modelEnabled:has('button[aria-label="Model"]') && !query('button[aria-label="Model"]')?.disabled,effort:has('button[aria-label="Thinking level"]'),add:has('button[aria-label="Composer options"]'),access:has('[aria-label="Permission mode"]'),stop:has('button[aria-label="Cancel turn"]'),send:has('button[aria-label="Send message"]'),meter:has('.composer__context [role="progressbar"]')};
  const assistantRendered=(${assistantTextRendered.toString()})(turn?.id,{querySelectorAll:queryAll},visible);
  return {chatId,projectId,projectionLoaded,singlePane,dpr:devicePixelRatio,css:{width:innerWidth,height:innerHeight},selected,bindingId:session?.bindingId ?? harness?.bindingId ?? null,turnId:turn?.id ?? 'none',turnCount:turns.length,failedTurns:turns.filter(turn=>turn.state==='failed').length,state:turn?.state ?? 'none',assistantRendered,itemKinds:(turn?.items ?? []).slice(0,64).map(item=>item.kind),toolStates:(turn?.items ?? []).filter(item=>item.kind==='toolCall').slice(0,64).map(item=>item.toolState),permissionStates:(turn?.items ?? []).filter(item=>item.kind==='permissionRequest').slice(0,16).map(item=>item.payload.state),noticeKinds:(turn?.items ?? []).filter(item=>item.kind==='unsupported').slice(0,32).map(item=>item.payload.category),compactionStates:(turn?.items ?? []).filter(item=>item.kind==='compactionNotice').map(item=>item.lifecycle),visible:Object.fromEntries(Object.entries(selectors).map(([kind,selector])=>[kind,has(selector)])),elements,controls,overflow:{horizontal:document.documentElement.scrollWidth>innerWidth}};
})()`;
}

export function freshEmptyChat(previous, observation) {
  return Boolean(
    previous.chatId &&
    previous.projectId &&
    observation.chatId &&
    observation.chatId !== previous.chatId &&
    observation.projectId === previous.projectId &&
    observation.projectionLoaded === true &&
    observation.turnCount === 0 &&
    observation.state === "none",
  );
}

/** Turn states from src/domain/acp.ts TurnState. */
export const LIVE_TURN_STATES = Object.freeze([
  "running",
  "waitingModel",
  "waitingTool",
  "waitingApproval",
  "waitingUser",
]);
export const SETTLED_TURN_STATES = Object.freeze([
  "completed",
  "interrupted",
  "cancelled",
  "failed",
  "deliveryUnknown",
]);
export function liveTurnReady(observation, turnId, bindingId) {
  return (
    observation.turnId === turnId &&
    observation.bindingId === bindingId &&
    LIVE_TURN_STATES.includes(observation.state) &&
    observation.controls.stop === true &&
    observation.assistantRendered === true
  );
}
export function liveActionExpression(observation, selector, turnId, bindingId) {
  if (!IDENTIFIER.test(turnId)) throw new Error("turn-identity");
  return `(async()=>{
    const latest=await ${observation};
    if(latest.turnId!==${JSON.stringify(turnId)} || latest.bindingId!==${JSON.stringify(bindingId)})return 'turn-identity-drift';
    if(${JSON.stringify(SETTLED_TURN_STATES)}.includes(latest.state))return 'turn-action-settled';
    const LIVE_TURN_STATES=${JSON.stringify(LIVE_TURN_STATES)};
    if(!(${liveTurnReady.toString()})(latest,${JSON.stringify(turnId)},${JSON.stringify(bindingId)}))return false;
    return ${clickExpression(selector)};
  })()`;
}
export function namedState(options, observation, captureChat) {
  const state = options.state,
    view = observation.visible,
    terminal = observation.state === "completed",
    live = LIVE_TURN_STATES.includes(observation.state),
    rendered = (kind) => observation.itemKinds.includes(kind);
  const predicates = {
    binding: observation.bindingId === HARNESS[options.harness]?.binding,
    selected: observation.selected === HARNESS[options.harness]?.id,
  };
  if (captureChat) {
    predicates.chat =
      observation.chatId === captureChat.chatId &&
      observation.projectId === captureChat.projectId &&
      observation.projectionLoaded === true;
    predicates.singlePane = observation.singlePane === true;
  }
  const rules = {
    "new-chat": observation.state === "none",
    "turn-running": live,
    "turn-completed": terminal,
    "worked-expanded": terminal && view.worked,
    "jump-to-latest": view.jump,
    "thinking-first-token": live && view.thought,
    "thinking-live": live && view.thought,
    "prose-then-thinking": live && view.thought && rendered("text"),
    "tool-running":
      live &&
      (view.tool || view.rollup) &&
      observation.toolStates.includes("inProgress"),
    "tool-finished":
      terminal &&
      (view.tool || view.rollup) &&
      observation.toolStates.includes("completed"),
    "tools-grouped":
      terminal && view.rollup && observation.toolStates.length >= 3,
    "tool-expanded": view.expanded,
    "tool-long-label": terminal && (view.tool || view.rollup),
    "file-edit-live": live && view.files && rendered("fileChangeDiff"),
    "files-pill": terminal && view.files,
    "file-hunk": terminal && view.hunk,
    "file-diff-tab": terminal && view.diffTab,
    "git-running": live && (view.tool || view.rollup),
    "git-finished": terminal && (view.tool || view.rollup),
    "subagents-started": live && view.subagent,
    "subagents-finished": terminal && view.subagent,
    "subagents-grouped": terminal && view.subagent,
    "subagents-popover": view.popover,
    "child-opened": view.child,
    "plan-running": live && view.plan,
    "plan-completed": terminal && view.plan,
    "plan-mode-entry": view.addMenu && view.planEntry,
    "compaction-completed":
      terminal &&
      view.compaction &&
      observation.compactionStates?.includes("completed"),
    "compaction-started":
      options.harness === "pi" &&
      live &&
      view.compaction &&
      observation.compactionStates?.includes("started"),
    "compaction-failed":
      options.harness === "pi" &&
      terminal &&
      view.compaction &&
      observation.compactionStates?.includes("failed"),
    "compaction-cancelled":
      options.harness === "pi" &&
      terminal &&
      view.compaction &&
      observation.compactionStates?.includes("cancelled"),
    "non-read-denial":
      options.harness === "pi" &&
      ["cancelled", "interrupted"].includes(observation.state) &&
      (view.tool || view.rollup) &&
      observation.toolStates.includes("declined"),
    "cancelled-dialog":
      options.harness === "pi" &&
      terminal &&
      view.notice &&
      observation.noticeKinds?.includes("pi_dialog_cancelled"),
    "approval-pending": live && view.pendingApproval,
    "approval-approved":
      terminal &&
      view.permission &&
      !view.pendingApproval &&
      observation.permissionStates?.includes("resolvedAllowed"),
    "approval-declined":
      terminal &&
      view.permission &&
      !view.pendingApproval &&
      observation.permissionStates?.includes("resolvedDeclined"),
    "turn-error": observation.state === "failed",
    "turn-stopped": ["cancelled", "interrupted"].includes(observation.state),
    "turn-retry":
      options.harness === "pi"
        ? terminal &&
          observation.turnCount === 1 &&
          observation.failedTurns === 0 &&
          view.answer &&
          observation.elements?.some(
            (element) =>
              element.kind === "answer" &&
              element.text.includes("Synthetic retry completed."),
          )
        : terminal &&
          observation.turnCount >= 2 &&
          observation.failedTurns >= 1,
    "answer-table-lists": terminal && view.table,
    "answer-code": terminal && view.code,
    "answer-mermaid": terminal && view.mermaid,
    "answer-image": terminal && view.image,
    "composer-idle": !observation.controls.stop && observation.controls.send,
    "composer-stop": live && observation.controls.stop,
    "harness-menu": view.harnessMenu,
    "model-menu": view.modelMenu,
    "effort-menu": view.effortMenu,
    "add-menu": view.addMenu,
    "access-menu": view.addMenu && view.accessMenu,
    "context-meter": view.meter,
    "responsive-wide": terminal && observation.controls.send,
    "responsive-720": terminal && observation.controls.send,
    "responsive-480": terminal && observation.controls.send,
  };
  predicates.namedState = rules[state] === true;
  return predicates;
}
export function clickExpression(selector) {
  return `(()=>{const paneSelector=${paneSelector.toString()};const ownedPaneNodes=${ownedPaneNodes.toString()};const elements=ownedPaneNodes(document,${JSON.stringify(selector)});if(elements.length!==1 || elements[0].disabled)return false;elements[0].scrollIntoView({block:'center',inline:'nearest'});elements[0].click();return true})()`;
}
/** Opens the single matching disclosure summary; idempotent, true once open. */
export function openExpression(selector) {
  return `(()=>{const paneSelector=${paneSelector.toString()};const ownedPaneNodes=${ownedPaneNodes.toString()};const elements=ownedPaneNodes(document,${JSON.stringify(selector)});const details=elements[0]?.parentElement;if(elements.length!==1 || details?.tagName!=='DETAILS')return false;if(!details.open){elements[0].scrollIntoView({block:'center',inline:'nearest'});elements[0].click()}return details.open})()`;
}
export function isDisclosure(selector) {
  return selector.endsWith("> summary");
}
export function isLiveTurnAction(selector) {
  return (
    selector === 'button[aria-label="Cancel turn"]' ||
    selector.includes(".acp-permission__actions button:")
  );
}
const LAST_TURN = ".acp-turn:last-of-type ";
/**
 * The project welcome turn precedes the replayed one, and the newest section can
 * render after the projection settles; pin latest-turn actions to its identity.
 */
export function scopeToTurn(selector, turnId) {
  if (!selector.startsWith(LAST_TURN)) return selector;
  if (!IDENTIFIER.test(turnId)) throw new Error("turn-identity");
  return (
    '.acp-turn[data-turn-id="' +
    turnId +
    '"] ' +
    selector.slice(LAST_TURN.length)
  );
}
export function actionsFor(state, harness) {
  if (state === "turn-retry" && harness === "pi") return [];
  if (
    [
      "worked-expanded",
      "tool-finished",
      "tool-expanded",
      "tools-grouped",
      "tool-long-label",
      "git-finished",
      "plan-completed",
      "answer-image",
      "compaction-completed",
      "compaction-failed",
      "compaction-cancelled",
      "non-read-denial",
      "cancelled-dialog",
      "subagents-finished",
      "subagents-grouped",
      "subagents-popover",
      "child-opened",
    ].includes(state)
  )
    return [
      LAST_TURN + ".chat-transcript-work > summary",
      ...(state === "tool-expanded"
        ? [LAST_TURN + ".acp-tool-rollup > summary"]
        : []),
      ...(state === "cancelled-dialog"
        ? [LAST_TURN + ".acp-unknown .acp-details > summary"]
        : []),
      ...(["subagents-popover", "child-opened"].includes(state)
        ? [LAST_TURN + ".acp-subagent-row"]
        : []),
    ];
  if (state === "approval-approved")
    return [
      LAST_TURN + ".acp-permission__actions button:first-child",
      LAST_TURN + ".chat-transcript-work > summary",
    ];
  if (state === "approval-declined")
    return [
      LAST_TURN + ".acp-permission__actions button:last-child",
      LAST_TURN + ".chat-transcript-work > summary",
    ];
  if (state === "turn-stopped") return ['button[aria-label="Cancel turn"]'];
  if (state === "turn-retry") return [".acp-turn__failure button"];
  if (state === "file-hunk" || state === "file-diff-tab")
    return [
      ".acp-file-changes__header",
      state === "file-hunk"
        ? '.acp-file-changes button[aria-label^="Preview"]'
        : '.acp-file-changes button[aria-label^="Open diff"]',
    ];
  if (state === "plan-mode-entry" || state === "access-menu")
    return [
      'button[aria-label="Composer options"]',
      state === "plan-mode-entry"
        ? 'button[aria-label="Session mode"]'
        : 'button[aria-label="Permission mode"]',
    ];
  const menus = {
    "harness-menu": "Harness",
    "model-menu": "Model",
    "effort-menu": "Thinking level",
    "add-menu": "Composer options",
    "access-menu": "Composer options",
    "plan-mode-entry": "Composer options",
  };
  return menus[state] ? ['button[aria-label="' + menus[state] + '"]'] : [];
}

import { IDENTIFIER } from "./capture-native-chat-shared.mjs";
import { richContentReadiness } from "./capture-native-chat-rich-content.mjs";
import {
  paneSelector,
  ownedPaneNodes,
  SINGLE_CAPTURE_PANE,
} from "./capture-native-chat-pane.mjs";
export const IDLE_CAPTURE_STATES = Object.freeze([
  "new-chat",
  "composer-idle",
  "harness-menu",
]);
/** Composer controls enable only once the owned Turn settles (CP-29d/e). */
export const COMPOSER_IDLE_CONTROL = Object.freeze({
  "model-menu": "modelEnabled",
  "effort-menu": "effortEnabled",
  "add-menu": "addEnabled",
  "plan-mode-entry": "addEnabled",
  "access-menu": "accessEnabled",
});
/** CP-29e: one composer action menu; session modes are a group inside it. */
const COMPOSER_MENU = '[role="menu"][aria-label="Composer options menu"]';
/** CP-30c chat-wide edit summary above the composer. */
const FILES_EDITED = 'section[aria-label="Files edited in this chat"]';
export const FILES_EDITED_HEADER = FILES_EDITED + " > button[aria-expanded]";
/** Kinds whose matching node must also carry this accessible name. */
export const NAMED_KINDS = Object.freeze({ planEntry: "Plan" });
export function accessibleName(element) {
  return (
    element.getAttribute("aria-label") ??
    element.textContent ??
    ""
  ).trim();
}
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
export function hasVisibleLazyFallback(document, visible) {
  return Array.from(
    document.querySelectorAll(
      '.acp-chat-projection [role="status"], .chat-surface [role="status"]',
    ),
  ).some(
    (element) =>
      visible(element) &&
      /^Loading\s+.+(?:\u2026|\.\.\.)$/u.test(element.textContent.trim()),
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
  const harness=store.harnesses?.find(harness=>harness.id===selected && harness.bindingId===expectedBinding);
  const accessModes=(harness?.accessModes ?? []).slice(0,3).map(mode=>mode.id);
  const sessionOf=turn=>page?.sessions?.find(session=>session.id===turn?.sessionId);
  const owned=turns.filter(turn=>sessionOf(turn)?.bindingId===expectedBinding);
  const turn=owned.at(-1) ?? turns.at(-1), session=sessionOf(turn);
  const visibleRectangle=${visibleRectangle.toString()};
  const hiddenByClosedDetails=${hiddenByClosedDetails.toString()};
  const visible=element=>Boolean(element && element.getClientRects().length && visibleRectangle(element.getBoundingClientRect(),{width:innerWidth,height:innerHeight}) && getComputedStyle(element).visibility !== "hidden" && !hiddenByClosedDetails(element));
  const has=selector=>queryAll(selector).some(visible);
  const selectors={thought:'.acp-thought',tool:'.acp-tool-card',rollup:'.acp-tool-rollup',files:'.chat-edit-summary-zone .acp-file-changes__header',editingFiles:'.acp-editing-files, .acp-tool-rollup[data-activity-kind="edit"][data-activity-state="inProgress"]',hunk:'${FILES_EDITED} [role="table"][aria-label="Inline saved diff"]',diffTab:'[aria-label="Agent artifact tabs"] [role="tab"][aria-label^="Diff "]',diffView:'[aria-label="Agent artifacts"] [role="table"][aria-label="Unified file diff"]',subagent:'.acp-subagent-row, [data-activity-kind="subagent"]',popover:'[role="dialog"][aria-label="Chat resources"]',subagentCounts:'[role="dialog"][aria-label="Chat resources"] [aria-label^="Recent subagents working"]',subagentEntries:'[role="dialog"][aria-label="Chat resources"] [aria-label="Subagents"] .agent-task-header__agent-list .agent-task-header__agent',child:'.subagent-surface__transcript .acp-message',plan:'.acp-plan',compaction:'.acp-compaction-notice',notice:'.acp-unknown',permission:'.acp-permission',pendingApproval:'.acp-permission__actions',error:'.acp-error',failure:'.acp-turn__failure',stopped:'.acp-turn__reason[role="status"]',answer:'.acp-markdown',table:'.acp-markdown table',code:'.acp-code-block',mermaid:'.acp-mermaid',image:'.acp-image',worked:'.chat-transcript-work[open]',expanded:'.acp-tool-rollup__items',jump:'.chat-follow-end',modelMenu:'.composer__model-menu',harnessMenu:'.composer__harness-menu',effortMenu:'.composer__effort-menu[role="dialog"]',effortSlider:'.composer__effort-menu input[type="range"][aria-label="Thinking level"]',addMenu:'${COMPOSER_MENU}',access:'[aria-label="Access options"]',accessMenu:'[role="menu"][aria-label="Access mode"]',planEntry:'${COMPOSER_MENU} [role="group"][aria-label="Session mode"] [role="menuitemradio"]',meter:'.composer__context [role="progressbar"]'};
  const currentTurn=queryAll('.acp-turn').find(element=>turn?.id && element.dataset.turnId===turn.id);
  const turnKinds=new Set(['thought','tool','rollup','editingFiles','subagent','plan','compaction','notice','permission','pendingApproval','error','failure','stopped','answer','table','code','mermaid','image','worked','expanded']);
  const namedKinds=${JSON.stringify(NAMED_KINDS)};
  const accessibleName=${accessibleName.toString()};
  const named=(kind,nodes)=>namedKinds[kind] ? nodes.filter(element=>accessibleName(element)===namedKinds[kind]) : nodes;
  const nodesFor=(kind,selector)=>named(kind,turnKinds.has(kind) ? queryAll(selector).filter(element=>currentTurn && element.closest('.acp-turn')===currentTurn) : queryAll(selector));
  const elements=[]; let charged=0;
  for(const [kind,selector] of Object.entries(selectors))for(const element of nodesFor(kind,selector)){
    if(elements.length>=64)throw Error('probe-elements');
    const raw=(element.innerText ?? '').trim(); charged+=raw.length;
    if(raw.length>16384 || charged>65536)throw Error('probe-text-bound');
    if(/sk-(?:ant-)?|secret-command|C:\\Users\\|\/home\/|\/Users\//i.test(raw))throw Error('privacy');
    const rectangle=element.getBoundingClientRect();
    if(!visible(element))continue;
    elements.push({kind,text:raw.slice(0,128),x:rectangle.x,y:rectangle.y,width:rectangle.width,height:rectangle.height});
  }
  const controls={harness:has('button[aria-label="Harness"]'),model:has('button[aria-label="Model"]'),modelEnabled:has('button[aria-label="Model"]') && !query('button[aria-label="Model"]')?.disabled,effort:has('button[aria-haspopup="dialog"][title="Select effort"]'),effortEnabled:has('button[aria-haspopup="dialog"][title="Select effort"]') && !query('button[aria-haspopup="dialog"][title="Select effort"]')?.disabled,add:has('button[aria-label="Composer options"]'),addEnabled:has('button[aria-label="Composer options"]') && !query('button[aria-label="Composer options"]')?.disabled,access:has('[aria-label="Access options"]'),accessEnabled:has('button[aria-label="Access options"]') && !query('button[aria-label="Access options"]')?.disabled,stop:has('button[aria-label="Cancel turn"]'),send:has('button[aria-label="Send message"]'),meter:has('.composer__context [role="progressbar"]')};
  const assistantRendered=(${assistantTextRendered.toString()})(turn?.id,{querySelectorAll:queryAll},visible);
  const lazyFallbackVisible=(${hasVisibleLazyFallback.toString()})({querySelectorAll:queryAll},visible);
  const richContent=(${richContentReadiness.toString()})(currentTurn,visible,element=>getComputedStyle(element));
  return {chatId,projectId,projectionLoaded,singlePane,accessModes,dpr:devicePixelRatio,css:{width:innerWidth,height:innerHeight},selected,bindingId:session?.bindingId ?? harness?.bindingId ?? null,turnId:turn?.id ?? 'none',turnCount:turns.length,failedTurns:turns.filter(turn=>turn.state==='failed').length,state:turn?.state ?? 'none',assistantRendered,lazyFallbackVisible,...richContent,itemKinds:(turn?.items ?? []).slice(0,64).map(item=>item.kind),toolStates:(turn?.items ?? []).filter(item=>item.kind==='toolCall').slice(0,64).map(item=>item.toolState),permissionStates:(turn?.items ?? []).filter(item=>item.kind==='permissionRequest').slice(0,16).map(item=>item.payload.state),noticeKinds:(turn?.items ?? []).filter(item=>item.kind==='unsupported').slice(0,32).map(item=>item.payload.category),compactionStates:(turn?.items ?? []).filter(item=>item.kind==='compactionNotice').map(item=>item.lifecycle),visible:Object.fromEntries(Object.entries(selectors).map(([kind,selector])=>[kind,nodesFor(kind,selector).some(visible)])),elements,controls,overflow:{horizontal:document.documentElement.scrollWidth>innerWidth}};
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
    assistantRendered = observation.assistantRendered === true,
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
    "turn-completed": terminal && assistantRendered,
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
    "file-edit-live": live && view.editingFiles && rendered("fileChangeDiff"),
    "files-pill": terminal && view.files,
    "file-hunk": terminal && view.hunk,
    // The lazy Diff surface first shows "Loading Diff…"; wait for its table.
    "file-diff-tab": terminal && view.diffTab && view.diffView,
    "git-running": live && (view.tool || view.rollup),
    "git-finished": terminal && (view.tool || view.rollup),
    "subagents-started": live && view.subagent,
    "subagents-finished": terminal && view.subagent,
    "subagents-grouped": terminal && view.subagent,
    "subagents-popover":
      terminal && view.popover && view.subagentCounts && view.subagentEntries,
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
    "turn-error":
      observation.state === "failed" && (assistantRendered || view.failure),
    "turn-stopped":
      ["cancelled", "interrupted"].includes(observation.state) &&
      (assistantRendered || view.stopped),
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
          observation.failedTurns >= 1 &&
          assistantRendered,
    "answer-table-lists": terminal && view.table,
    "answer-code":
      terminal && view.code && observation.codeHighlighted === true,
    "answer-mermaid": terminal && view.mermaid,
    "answer-image": terminal && view.image && observation.imageLoaded === true,
    "composer-idle": !observation.controls.stop && observation.controls.send,
    "composer-stop": live && observation.controls.stop,
    "harness-menu": view.harnessMenu,
    "model-menu": view.modelMenu,
    "effort-menu":
      terminal && assistantRendered && view.effortMenu && view.effortSlider,
    "add-menu": view.addMenu,
    "access-menu": view.accessMenu,
    "context-meter": view.meter,
    "responsive-wide":
      terminal && assistantRendered && observation.controls.send,
    "responsive-720":
      terminal && assistantRendered && observation.controls.send,
    "responsive-480":
      terminal && assistantRendered && observation.controls.send,
  };
  predicates.namedState =
    rules[state] === true && observation.lazyFallbackVisible === false;
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
/** Toggle buttons that default open must not be clicked closed. */
export function isExpandable(selector) {
  return (
    selector === FILES_EDITED_HEADER ||
    selector.includes('[aria-label^="Preview saved diff"]')
  );
}
/** Expands the single matching aria-expanded button; idempotent, true once open. */
export function expandExpression(selector) {
  return `(()=>{const paneSelector=${paneSelector.toString()};const ownedPaneNodes=${ownedPaneNodes.toString()};const elements=ownedPaneNodes(document,${JSON.stringify(selector)});const button=elements[0];if(elements.length!==1 || button.disabled || !button.hasAttribute('aria-expanded'))return false;if(button.getAttribute('aria-expanded')!=='true'){button.scrollIntoView({block:'center',inline:'nearest'});button.click()}return button.getAttribute('aria-expanded')==='true'})()`;
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
  if (state === "subagents-popover")
    return ['button[aria-label="Show chat resources"]'];
  if (state === "effort-menu")
    return ['button[aria-haspopup="dialog"][title="Select effort"]'];
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
      "child-opened",
    ].includes(state)
  )
    return [
      LAST_TURN + ".chat-transcript-work > summary",
      ...(state === "tool-expanded"
        ? [LAST_TURN + ".acp-tool-rollup > summary"]
        : []),
      ...(state === "child-opened" ? [LAST_TURN + ".acp-subagent-row"] : []),
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
      FILES_EDITED_HEADER,
      state === "file-hunk"
        ? FILES_EDITED + ' button[aria-label^="Preview saved diff"]'
        : FILES_EDITED + ' button[aria-label^="Open diff"]',
    ];
  if (state === "access-menu") return ['button[aria-label="Access options"]'];
  const menus = {
    "harness-menu": "Harness",
    "model-menu": "Model",
    "add-menu": "Composer options",
    "plan-mode-entry": "Composer options",
  };
  return menus[state] ? ['button[aria-label="' + menus[state] + '"]'] : [];
}

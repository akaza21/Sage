import { NATIVE_IMAGE_BASE64 } from "./acp-native-case-events.mjs";
export const HARNESS = Object.freeze({
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
export const IMAGE_ATTACHMENT = String.raw`(()=>{const input=document.querySelector('input[aria-label="Choose images"]');if(!input || input.disabled)return false;const transfer=new DataTransfer();transfer.items.add(new File([Uint8Array.from(atob(${JSON.stringify(NATIVE_IMAGE_BASE64)}),character=>character.charCodeAt(0))],"synthetic-replay.png",{type:"image/png"}));input.files=transfer.files;input.dispatchEvent(new Event("change",{bubbles:true}));return true})()`;

export const OBSERVATION = String.raw`(async()=>{
  const {useWorkbenchStore}=await import('/src/app/store.ts');
  const {acpProjectionStoreFor}=await import('/src/app/acpProjectionRegistry.ts');
  const store=useWorkbenchStore.getState(), page=acpProjectionStoreFor(store.activeTabId).getState().page;
  const turns=page?.turns ?? [];
  if(turns.length>64)throw Error('projection-bound');
  const turn=turns.at(-1), session=page?.sessions?.find(session=>session.id===turn?.sessionId) ?? page?.sessions?.at(-1);
  const selected=document.querySelector('button[aria-label="Harness"]')?.getAttribute('data-selected-id');
  const harness=store.harnesses?.find(harness=>harness.id===selected);
  const visibleRectangle=${visibleRectangle.toString()};
  const visible=element=>Boolean(element && element.getClientRects().length && visibleRectangle(element.getBoundingClientRect(),{width:innerWidth,height:innerHeight}) && getComputedStyle(element).visibility !== "hidden" && !Array.from(element.closest("details:not([open])") ? [element.closest("details:not([open])")] : []).some(details=>!details.querySelector("summary")?.contains(element)));
  const has=selector=>Array.from(document.querySelectorAll(selector)).some(visible);
  const selectors={thought:'.acp-thought',tool:'.acp-tool-card',rollup:'.acp-tool-rollup',files:'.acp-file-changes',hunk:'.acp-inline-diff',diffTab:'.tab[data-resource-kind="diff"]',subagent:'.acp-subagent-row, [data-activity-kind="subagent"]',popover:'.subagent-surface',child:'.subagent-surface__transcript .acp-message',plan:'.acp-plan',compaction:'.acp-compaction-notice',permission:'.acp-permission',pendingApproval:'.acp-permission__actions',error:'.acp-error',table:'.acp-markdown table',code:'.acp-code-block',mermaid:'.acp-mermaid',image:'.acp-image',worked:'.chat-transcript-work[open]',expanded:'.acp-tool-rollup__items',jump:'.chat-follow-end',modelMenu:'.composer__model-menu',harnessMenu:'.composer__harness-menu',effortMenu:'.composer__effort-menu',addMenu:'.composer__options',access:'[aria-label="Permission mode"]',accessMenu:'[role="listbox"][aria-label="Permission mode"]',planEntry:'[role="listbox"][aria-label="Session mode"] [data-option-id="plan"]',meter:'.composer__context [role="progressbar"]',attachment:'.composer__attachment'};
  const elements=[]; let charged=0;
  for(const [kind,selector] of Object.entries(selectors))for(const element of document.querySelectorAll(selector)){
    if(elements.length>=64)throw Error('probe-elements');
    const raw=(element.innerText ?? '').trim(); charged+=raw.length;
    if(raw.length>16384 || charged>65536)throw Error('probe-text-bound');
    if(/sk-(?:ant-)?|secret-command|C:\\Users\\|\/home\/|\/Users\//i.test(raw))throw Error('privacy');
    const rectangle=element.getBoundingClientRect();
    if(!visible(element))continue;
    elements.push({kind,text:raw.slice(0,128),x:rectangle.x,y:rectangle.y,width:rectangle.width,height:rectangle.height});
  }
  const controls={harness:has('button[aria-label="Harness"]'),model:has('button[aria-label="Model"]'),effort:has('button[aria-label="Thinking level"]'),add:has('button[aria-label="Composer options"]'),access:has('[aria-label="Permission mode"]'),stop:has('button[aria-label="Cancel turn"]'),send:has('button[aria-label="Send message"]'),attachment:has('.composer__attachment'),meter:has('.composer__context [role="progressbar"]')};
  return {dpr:devicePixelRatio,css:{width:innerWidth,height:innerHeight},selected,bindingId:session?.bindingId ?? harness?.bindingId ?? null,turnId:turn?.id ?? 'none',turnCount:turns.length,failedTurns:turns.filter(turn=>turn.state==='failed').length,state:turn?.state ?? 'none',itemKinds:(turn?.items ?? []).slice(0,64).map(item=>item.kind),toolStates:(turn?.items ?? []).filter(item=>item.kind==='toolCall').slice(0,64).map(item=>item.toolState),permissionStates:(turn?.items ?? []).filter(item=>item.kind==='permissionRequest').slice(0,16).map(item=>item.payload.state),compactionStates:(turn?.items ?? []).filter(item=>item.kind==='compactionNotice').map(item=>item.lifecycle),visible:Object.fromEntries(Object.entries(selectors).map(([kind,selector])=>[kind,has(selector)])),elements,controls,overflow:{horizontal:document.documentElement.scrollWidth>innerWidth}};
})()`;

export function namedState(options, observation) {
  const state = options.state,
    view = observation.visible,
    terminal = observation.state === "completed",
    live = observation.state === "running",
    rendered = (kind) => observation.itemKinds.includes(kind);
  const predicates = {
    binding: observation.bindingId === HARNESS[options.harness]?.binding,
    selected: observation.selected === HARNESS[options.harness]?.id,
  };
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
      terminal && observation.turnCount >= 2 && observation.failedTurns >= 1,
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
    "attachment-chip": view.attachment,
    "responsive-wide": terminal && observation.controls.send,
    "responsive-720": terminal && observation.controls.send,
    "responsive-480": terminal && observation.controls.send,
  };
  predicates.namedState = rules[state] === true;
  return predicates;
}
export function clickExpression(selector) {
  return (
    "(()=>{const elements=[...document.querySelectorAll(" +
    JSON.stringify(selector) +
    ")];if(elements.length!==1 || elements[0].disabled)return false;elements[0].scrollIntoView({block:'center',inline:'nearest'});elements[0].click();return true})()"
  );
}
export function actionsFor(state) {
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
      "subagents-finished",
      "subagents-grouped",
      "subagents-popover",
      "child-opened",
    ].includes(state)
  )
    return [
      ".chat-transcript-work > summary",
      ...(state === "tool-expanded" ? [".acp-tool-rollup > summary"] : []),
      ...(["subagents-popover", "child-opened"].includes(state)
        ? [".acp-subagent-row"]
        : []),
    ];
  if (state === "approval-approved")
    return [
      ".acp-permission__actions button:first-child",
      ".acp-turn:last-of-type .chat-transcript-work > summary",
    ];
  if (state === "approval-declined")
    return [
      ".acp-permission__actions button:last-child",
      ".acp-turn:last-of-type .chat-transcript-work > summary",
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

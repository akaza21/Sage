// Observation selectors, named-state rules, chat ownership and driver steps
// for the waves 6-8 parity states (capture-native-chat-parity.mjs). Product
// anchors are the current components' accessible names and classes; the
// catalog test pins them to the source.
import { parityOwner } from "./capture-native-chat-parity.mjs";

const SIDE_CHAT_TAB = ".agent-chat-center__side-tab .side-chat-tab";

/** Pane-owned witnesses (queried inside the focused pane and its owned roots). */
export const PARITY_PANE_SELECTORS = Object.freeze({
  sideChatHome: ".side-chat-home",
  draftChip: ".composer__annotation-row .annotation-chip--draft",
  sentChip: ".annotation-chip--sent",
  navigatorActive: '.turn-navigator button[data-active="true"]',
  navigatorPreview: '.turn-navigator__preview[role="tooltip"]',
  findBar: '.chat-find[role="search"]',
  toolResult: ".acp-tool-result .acp-code-block",
  toolError: '.acp-tool-error[role="alert"]',
  // Captured opened, as s5/f0405 and s2/f0015 show them.
  webSearch: '.acp-tool-rollup[data-activity-kind="webSearch"][open]',
  mixedGroup:
    '.acp-tool-rollup[data-activity-kind="mixed"][open] > .acp-tool-rollup__items',
  approvalPrompt: '.acp-permission-prompt[aria-label="Permission request"]',
  composerInput: 'textarea[aria-label="Message agent"]',
  subagentName: ".acp-subagent-row .acp-subagent-name",
  childHeader: ".subagent-surface__bar h1",
  childBack: 'button[aria-label="Back to subagents"]',
  subagentActive: 'section[aria-label="Active subagents"]',
  subagentDone: 'section[aria-label="Done subagents"]',
  moreActions: 'button[aria-label="More actions"]',
  forkDivider: ".chat-fork-divider",
  shimmer: ".acp-live-shimmer",
  workedClosed: "details.chat-transcript-work:not([open])",
});
/** Witnesses that must belong to the observed Turn. */
export const PARITY_TURN_KINDS = Object.freeze([
  "toolResult",
  "toolError",
  "webSearch",
  "mixedGroup",
  "subagentName",
  "workedClosed",
]);
/**
 * Body-portaled surfaces (TranscriptInteractions, ModalDialog, MenuPopover).
 * Only the focused pane shows the selection popover and comment field. A
 * side chat's tab sits in the Agent center header row, outside every pane
 * (w9int); a Project owns at most one side chat, so its tab is unambiguous.
 */
export const PARITY_DOCUMENT_SELECTORS = Object.freeze({
  sideChatTab: SIDE_CHAT_TAB,
  selectionPopover: '.chat-selection-popover[role="toolbar"]',
  annotationComment: 'form.annotation-comment[aria-label="Annotation comment"]',
  annotationBadge: ".transcript-annotation-badge",
  closeSideDialog: "dialog.close-side-chat-dialog[open]",
  forkMenuItem: '.answer-more-menu [role="menuitem"]',
  forkDialog: "dialog.fork-chat-dialog[open]",
});

/** Page-side extra witnesses; serialized into the observation expression. */
export function parityObservation(document, queryAll, visible, context) {
  const panes = document.querySelectorAll(".agent-chat-pane");
  const focused = document.querySelectorAll(".agent-chat-pane--focused");
  if (panes.length > 8 || focused.length > 8)
    throw Error("capture-owner-bound");
  const names = queryAll(".acp-subagent-row .acp-subagent-name").filter(
    visible,
  );
  const status = queryAll('.chat-find [role="status"]').find(visible);
  const animations =
    typeof document.getAnimations === "function"
      ? document.getAnimations()
      : [];
  if (animations.length > 4096) throw Error("probe-animation-bound");
  return {
    paneCount: panes.length,
    focusedPaneCount: focused.length,
    sideParentChatId: context.sideParentChatId ?? null,
    forkedFrom: context.forkedFrom ?? null,
    hoveredSubagentName: names.findIndex((name) => name.matches(":hover")),
    findStatus: (status?.textContent ?? "").trim().slice(0, 64),
    streamAnimations: animations.filter(
      (animation) => animation.animationName === "acp-stream-in",
    ).length,
  };
}

const FIND_RESULTS = /^1 \/ [1-9][0-9]{0,2}\+? results?$/u;

/** Named-state rules for parity states, merged into namedState. */
export function parityRules(
  observation,
  { terminal, live, assistantRendered },
) {
  const view = observation.visible;
  const toolStates = observation.toolStates ?? [];
  return {
    "selection-popover": terminal && assistantRendered && view.selectionPopover,
    "annotation-field":
      terminal &&
      view.annotationComment &&
      view.annotationBadge &&
      view.draftChip,
    "annotation-chip":
      terminal &&
      !view.annotationComment &&
      view.annotationBadge &&
      view.draftChip,
    "annotation-sent":
      terminal &&
      assistantRendered &&
      observation.turnCount >= 2 &&
      view.sentChip &&
      !view.annotationBadge,
    "side-chat-open":
      observation.state === "none" &&
      view.sideChatTab &&
      view.sideChatHome &&
      view.draftChip,
    "side-chat-close-confirm": view.sideChatTab && view.closeSideDialog,
    "turn-navigator-hover":
      terminal &&
      observation.turnCount >= 2 &&
      view.navigatorActive &&
      view.navigatorPreview,
    "find-in-chat":
      terminal &&
      view.findBar &&
      FIND_RESULTS.test(observation.findStatus ?? ""),
    "tool-mcp-result":
      terminal && view.toolResult && toolStates.includes("completed"),
    "tool-mcp-error":
      terminal && view.toolError && toolStates.includes("failed"),
    "tool-web-search": terminal && view.webSearch,
    "tool-mixed-group": terminal && view.mixedGroup,
    "approval-card": live && view.approvalPrompt && !view.composerInput,
    "subagents-name-hover":
      terminal && view.subagentName && observation.hoveredSubagentName === 1,
    "subagents-tab-active-done": view.subagentActive && view.subagentDone,
    "subagent-child-header": view.childHeader && view.childBack,
    "fork-menu": terminal && view.forkMenuItem,
    "fork-dialog": terminal && view.forkDialog,
    "fork-thread": view.forkDivider && Boolean(observation.forkedFrom),
    // Codex shimmers live labels (s8: a "Running …" row); Sage's live
    // reasoning prose never shimmers, so the witness is a live tool label.
    "motion-thinking-shimmer":
      live && view.shimmer && (view.tool || view.rollup),
    "motion-stream-fade":
      live && assistantRendered && observation.streamAnimations > 0,
    "motion-worked-expand": terminal && view.workedClosed,
    "motion-jump-pill": terminal && view.jump,
  };
}

/**
 * The capture Chat, or for side/fork states the Chat the drive opens from
 * it: a temporary side chat whose parent is the capture Chat, or a fork
 * whose lineage names it.
 */
export function parityChatPredicates(state, observation, captureChat) {
  const owner = parityOwner(state);
  const loaded =
    observation.projectId === captureChat.projectId &&
    observation.projectionLoaded === true;
  if (owner === "side")
    return {
      chat:
        loaded &&
        /^side-/u.test(observation.chatId ?? "") &&
        observation.sideParentChatId === captureChat.chatId,
      // The side chat pairs with its parent, or shows alone in compact layouts.
      singlePane:
        observation.paneCount >= 1 &&
        observation.paneCount <= 2 &&
        observation.focusedPaneCount === 1,
    };
  if (owner === "fork")
    return {
      chat:
        loaded &&
        Boolean(observation.chatId) &&
        observation.chatId !== captureChat.chatId &&
        observation.forkedFrom === captureChat.chatId,
      singlePane: observation.singlePane === true,
    };
  return {
    chat: observation.chatId === captureChat.chatId && loaded,
    singlePane: observation.singlePane === true,
  };
}

const LAST = ".acp-turn:last-of-type ";
const WORKED = LAST + ".chat-transcript-work > summary";
const POPOVER = ".chat-selection-popover";
export const SELECTED_PHRASE = "synthetic example";
export const FIND_QUERY = "synthetic";
export const ANNOTATION_COMMENT = "Does this hold for every harness?";
export const FORK_QUIET_MS = 600;

const select = { op: "select", phrase: SELECTED_PHRASE };
const add = {
  op: "click",
  scope: "document",
  selector: POPOVER + " > button:first-of-type",
};
const comment = [
  {
    op: "fill",
    scope: "document",
    selector: 'form.annotation-comment input[aria-label="Annotation comment"]',
    text: ANNOTATION_COMMENT,
  },
  {
    op: "click",
    scope: "document",
    selector: 'form.annotation-comment button[aria-label="Save annotation"]',
  },
];
const side = {
  op: "click",
  scope: "document",
  selector: POPOVER + " > button:last-of-type",
};
const more = { op: "click", selector: 'button[aria-label="More actions"]' };
const branch = {
  op: "click",
  scope: "document",
  selector: '.answer-more-menu [role="menuitem"]',
};

/**
 * Ordered drive steps after the first Turn was submitted. `settle` waits for
 * the observed Turn to settle; `open` opens a disclosure idempotently;
 * `forked` waits for the fork Chat to stay rendered for `quietMs`;
 * `select`/`hover` use real CDP pointer input; `submit` sends a second Turn.
 */
export function parityStepsFor(state) {
  const steps = {
    "selection-popover": [{ op: "settle" }, select],
    "annotation-field": [{ op: "settle" }, select, add],
    "annotation-chip": [{ op: "settle" }, select, add, ...comment],
    "annotation-sent": [
      { op: "settle" },
      select,
      add,
      ...comment,
      { op: "submit" },
      { op: "settle" },
    ],
    "side-chat-open": [{ op: "settle" }, select, side],
    "side-chat-close-confirm": [
      { op: "settle" },
      select,
      side,
      {
        op: "click",
        scope: "document",
        selector: SIDE_CHAT_TAB + ' button[aria-label="Close side chat"]',
      },
    ],
    "turn-navigator-hover": [
      { op: "settle" },
      { op: "submit" },
      { op: "settle" },
      { op: "hover", selector: ".turn-navigator li:first-child button" },
    ],
    "find-in-chat": [
      { op: "settle" },
      {
        op: "key",
        focus: 'textarea[aria-label="Message agent"]',
        key: "f",
        ctrl: true,
        expect: '.chat-find input[aria-label="Search chat"]',
      },
      {
        op: "fill",
        selector: '.chat-find input[aria-label="Search chat"]',
        text: FIND_QUERY,
      },
    ],
    "tool-mcp-result": [
      { op: "open", selector: WORKED },
      { op: "open", selector: LAST + ".acp-tool-rollup > summary" },
    ],
    "tool-mcp-error": [
      { op: "open", selector: WORKED },
      { op: "open", selector: LAST + ".acp-tool-rollup > summary" },
    ],
    "tool-web-search": [
      { op: "open", selector: WORKED },
      { op: "open", selector: LAST + ".acp-tool-rollup > summary" },
    ],
    "tool-mixed-group": [
      { op: "open", selector: WORKED },
      { op: "open", selector: LAST + ".acp-tool-rollup > summary" },
    ],
    "subagents-name-hover": [
      { op: "open", selector: WORKED },
      {
        op: "hover",
        selector: LAST + ".acp-subagent-row .acp-subagent-name:nth-of-type(2)",
      },
    ],
    "subagents-tab-active-done": [
      // One row names both children while one still works (s4/f0244); a
      // click admits one match, so open the first named child.
      {
        op: "click",
        selector: LAST + ".acp-subagent-row .acp-subagent-name:first-of-type",
      },
      {
        op: "click",
        scope: "owner",
        selector: 'button[aria-label="Back to subagents"]',
      },
    ],
    "subagent-child-header": [
      { op: "open", selector: WORKED },
      { op: "click", selector: LAST + ".acp-subagent-row .acp-subagent-name" },
    ],
    "fork-menu": [{ op: "settle" }, more],
    "fork-dialog": [{ op: "settle" }, more, branch],
    "fork-thread": [
      { op: "settle" },
      more,
      branch,
      {
        op: "click",
        scope: "document",
        selector: 'dialog.fork-chat-dialog [role="menuitem"]',
      },
      // The committed fork Chat activates, then reloads its projection once
      // (~300 ms of lazy fallback in WebView2); observe it after that.
      { op: "forked", quietMs: FORK_QUIET_MS },
    ],
    "motion-worked-expand": [{ op: "settle" }],
  };
  return steps[state] ?? [];
}

/** Locates the phrase inside the observed Turn's selectable answer. */
export function phraseRectExpression(turnId, phrase) {
  if (!/^[A-Za-z0-9_-]{1,128}$/u.test(turnId)) throw new Error("turn-identity");
  if (!/^[a-z ]{1,64}$/u.test(phrase)) throw new Error("phrase-invalid");
  return `(()=>{const turn=[...document.querySelectorAll('.agent-chat-pane--focused .acp-turn')].find(element=>element.dataset.turnId===${JSON.stringify(turnId)});const answer=turn?.querySelector('[data-selectable-answer]');if(!answer)return false;const walker=document.createTreeWalker(answer,NodeFilter.SHOW_TEXT);let visited=0;while(walker.nextNode()){if(++visited>4096)return false;const node=walker.currentNode;const index=node.data.indexOf(${JSON.stringify(phrase)});if(index<0)continue;const range=document.createRange();range.setStart(node,index);range.setEnd(node,index+${phrase.length});node.parentElement.scrollIntoView({block:'nearest',inline:'nearest',behavior:'instant'});const box=range.getBoundingClientRect();if(!(box.width>0&&box.height>0))return false;return {x:box.x,y:box.y,width:box.width,height:box.height}}return false})()`;
}

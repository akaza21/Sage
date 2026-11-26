import { IDENTIFIER } from "./capture-native-chat-shared.mjs";
import { paneSelector, ownedPaneNodes } from "./capture-native-chat-pane.mjs";
/**
 * The controls a capture drives and the in-page expressions that drive them.
 * The DOM observation module re-exports all of these for its importers.
 */
/** CP-30c chat-wide edit summary above the composer. */
export const FILES_EDITED = 'section[aria-label="Files edited in this chat"]';
export const FILES_EDITED_HEADER = FILES_EDITED + " > button[aria-expanded]";
/**
 * The thinking level is changed in the Model chip's "Select model" dialog
 * (ComposerModelControls); the chip shows the level it holds.
 */
export const EFFORT_TRIGGER =
  'button[aria-label="Model"][aria-haspopup="dialog"]';
/** The level the chip shows once the harness advertises levels. */
export const EFFORT_LEVEL = EFFORT_TRIGGER + " .composer__model-effort";
/** `chatId` also admits that Chat's owned artifact pane (Subagents, Diff). */
function singleEnabledControl(selector, chatId) {
  if (chatId !== null && !/^[A-Za-z0-9:_-]{1,128}$/u.test(chatId))
    throw new Error("chat-identity");
  return `const paneSelector=${paneSelector.toString()};const ownedPaneNodes=${ownedPaneNodes.toString()};const elements=ownedPaneNodes(document,${JSON.stringify(selector)},${JSON.stringify(chatId)});if(elements.length!==1 || elements[0].disabled)return false;`;
}
/** Clicks the single enabled owned match; false while none or several match. */
export function clickExpression(selector, chatId = null) {
  return `(()=>{${singleEnabledControl(selector, chatId)}elements[0].scrollIntoView({block:'center',inline:'nearest'});elements[0].click();return true})()`;
}
/** True exactly when clickExpression would click: one enabled owned match. */
export function clickableExpression(selector, chatId = null) {
  return `(()=>{${singleEnabledControl(selector, chatId)}return true})()`;
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
    selector.endsWith(".chat-transcript-work__toggle") ||
    selector.includes('[aria-label^="Preview saved diff"]')
  );
}
/** Expands the single matching aria-expanded button; idempotent, true once open. */
export function expandExpression(selector) {
  return `(()=>{const paneSelector=${paneSelector.toString()};const ownedPaneNodes=${ownedPaneNodes.toString()};const elements=ownedPaneNodes(document,${JSON.stringify(selector)});const button=elements[0];if(elements.length!==1 || button.disabled || !button.hasAttribute('aria-expanded'))return false;if(button.getAttribute('aria-expanded')!=='true'){button.scrollIntoView({block:'center',inline:'nearest'});button.click()}return button.getAttribute('aria-expanded')==='true'})()`;
}
export const APPROVAL_ALLOW =
  ".acp-permission-prompt__split > button:first-child";
export const APPROVAL_DENY =
  ".acp-permission-prompt__actions > button:not(.acp-permission-prompt__stop)";
export function isLiveTurnAction(selector) {
  return (
    selector === 'button[aria-label="Cancel turn"]' ||
    selector === APPROVAL_ALLOW ||
    selector === APPROVAL_DENY
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
  if (state === "effort-menu") return [EFFORT_TRIGGER];
  if (state === "turn-retry" && harness === "pi") return [];
  if (
    [
      "worked-expanded",
      "tool-expanded",
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
      LAST_TURN + ".chat-transcript-work__toggle",
      ...(state === "tool-expanded"
        ? [LAST_TURN + ".acp-tool-rollup > summary"]
        : []),
      ...(state === "child-opened"
        ? [LAST_TURN + ".acp-subagent-row .acp-subagent-name"]
        : []),
    ];
  // Wave 7: the pending approval replaces the composer (PermissionPrompt).
  if (state === "approval-approved") return [APPROVAL_ALLOW];
  if (state === "approval-declined") return [APPROVAL_DENY];
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

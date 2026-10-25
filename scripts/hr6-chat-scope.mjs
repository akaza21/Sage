import { boundedMatchProjection } from "./hr6-terminal-expression.mjs";

const TURN_SCOPE = "section.acp-turn";

function focusedPanesExpression() {
  return `(${boundedMatchProjection(2)})(document.documentElement,'section.agent-chat-pane.agent-chat-pane--focused',false,${JSON.stringify(TURN_SCOPE)})`;
}

function chatSurfacesExpression(paneExpression) {
  return `(${boundedMatchProjection(2)})(${paneExpression},'section.chat-surface[aria-label="Agent chat"]',false,${JSON.stringify(TURN_SCOPE)})`;
}

export function focusedChatSurfacePreamble(onInvalid) {
  return `
    const paneScan=${focusedPanesExpression()};
    if(paneScan.exceeded!==false||paneScan.nodes.length!==1){${onInvalid}}
    const surfaceScan=${chatSurfacesExpression("paneScan.nodes[0]")};
    if(surfaceScan.exceeded!==false||surfaceScan.nodes.length!==1){${onInvalid}}
    const surface=surfaceScan.nodes[0];
  `;
}

export function captureFocusedChatExpression(name, onInvalid) {
  return `
    const paneScan=${focusedPanesExpression()};
    if(paneScan.exceeded!==false||paneScan.nodes.length!==1){${onInvalid}}
    const chats=window.__hr6Chats??Object.create(null);
    if(chats[${JSON.stringify(name)}]){${onInvalid}}
    chats[${JSON.stringify(name)}]=paneScan.nodes[0];
    window.__hr6Chats=chats;
    return true;
  `;
}

export function rememberedChatSurfacePreamble(name, onInvalid) {
  return `
    const pane=window.__hr6Chats?.[${JSON.stringify(name)}];
    if(!pane||!pane.isConnected){${onInvalid}}
    const surfaceScan=${chatSurfacesExpression("pane")};
    if(surfaceScan.exceeded!==false||surfaceScan.nodes.length!==1){${onInvalid}}
    const surface=surfaceScan.nodes[0];
  `;
}

/**
 * Focuses a remembered Chat the way a user does: the only pane carrying its
 * `data-chat-id` takes focus through its composer, which the pane's focus
 * handler turns into the focused pane. Agent panes have no focus button.
 */
export function focusRememberedChatExpression(name, onInvalid) {
  return `
    const remembered=window.__hr6Chats?.[${JSON.stringify(name)}];
    if(!remembered||!remembered.isConnected){${onInvalid}}
    const chatId=remembered.getAttribute('data-chat-id');
    if(!chatId){${onInvalid}}
    const find=${boundedMatchProjection(2)};
    const panes=find(document.documentElement,'section.agent-chat-pane[data-chat-id='+JSON.stringify(chatId)+']',false,${JSON.stringify(TURN_SCOPE)});
    if(panes.exceeded!==false||panes.nodes.length!==1||panes.nodes[0]!==remembered){${onInvalid}}
    const prompts=find(remembered,'textarea[aria-label="Message agent"]',false,${JSON.stringify(TURN_SCOPE)});
    if(prompts.exceeded!==false||prompts.nodes.length!==1){${onInvalid}}
    prompts.nodes[0].focus();
    return true;
  `;
}

export function focusedRememberedChatSurfacePreamble(name, onInvalid) {
  return `
    ${rememberedChatSurfacePreamble(name, onInvalid)}
    const paneScan=${focusedPanesExpression()};
    if(paneScan.exceeded!==false||paneScan.nodes.length!==1||paneScan.nodes[0]!==pane){${onInvalid}}
  `;
}

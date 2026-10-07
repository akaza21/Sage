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

export function focusedRememberedChatSurfacePreamble(name, onInvalid) {
  return `
    ${rememberedChatSurfacePreamble(name, onInvalid)}
    const paneScan=${focusedPanesExpression()};
    if(paneScan.exceeded!==false||paneScan.nodes.length!==1||paneScan.nodes[0]!==pane){${onInvalid}}
  `;
}

export function paneSelector(selector) {
  if (
    [
      'button[aria-label="Open projects and chats"]',
      'button[aria-label="Close projects and chats"]',
      'button[aria-label="New task"]',
    ].includes(selector)
  )
    return selector;
  return selector
    .split(",")
    .map((part) => ".agent-chat-pane--focused " + part.trim())
    .join(", ");
}

export function ownedPaneNodes(document, selector, chatId = null) {
  const bounded = (matches, available) => {
    if (matches.length > available) throw new Error("capture-owner-bound");
    return matches;
  };
  const nodes = new Set(
    bounded(document.querySelectorAll(paneSelector(selector)), 64),
  );
  const controls = [
    ...bounded(document.querySelectorAll(paneSelector("[aria-controls]")), 64),
  ];
  const roots = [];
  if (chatId)
    for (const pane of bounded(
      document.querySelectorAll(".agent-artifact-pane"),
      32,
    ))
      if (pane.dataset.ownerChatId === chatId) roots.push(pane);
  for (let index = 0; index < controls.length; index++) {
    if (controls.length > 64 || roots.length > 32)
      throw new Error("capture-owner-bound");
    const references = controls[index].getAttribute?.("aria-controls") ?? "";
    if (references.length > 1024) throw new Error("capture-owner-bound");
    const ids = bounded(references.split(/\s+/u), 4);
    for (const id of ids) {
      const controlled = document.getElementById?.(id);
      const root = controlled?.closest(".menu-popover") ?? controlled;
      if (!root || roots.includes(root)) continue;
      if (roots.length >= 32) throw new Error("capture-owner-bound");
      roots.push(root);
      controls.push(
        ...bounded(
          root.querySelectorAll("[aria-controls]"),
          64 - controls.length,
        ),
      );
    }
  }
  for (const root of roots) {
    const add = (node) => {
      if (!nodes.has(node) && nodes.size >= 64)
        throw new Error("capture-owner-bound");
      nodes.add(node);
    };
    if (root.matches(selector)) add(root);
    for (const node of bounded(root.querySelectorAll(selector), 64)) add(node);
  }
  return [...nodes];
}

export function archivePreviousChatExpression(chatId) {
  if (!/^[A-Za-z0-9:_-]{1,128}$/u.test(chatId))
    throw new Error("chat-identity");
  return `(async()=>{const {useWorkbenchStore}=await import('/src/app/store.ts');return useWorkbenchStore.getState().archiveChat(${JSON.stringify(chatId)})})()`;
}

export const SINGLE_CAPTURE_PANE =
  "document.querySelectorAll('.agent-chat-pane').length===1 && document.querySelectorAll('.agent-chat-pane--focused').length===1";

export const SCROLL_TO_START = `(()=>{const transcript=document.querySelector('.agent-chat-pane--focused .message-list');if(!transcript || transcript.scrollHeight<=transcript.clientHeight)return false;transcript.scrollTop=0;transcript.dispatchEvent(new Event('scroll'));return true})()`;

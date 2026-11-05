// Back and Forward through the chats the Agent window has shown, as the
// Codex title bar offers. A chat is left behind when another one takes focus,
// in this project or another. History lives for the window session only.

import { create } from "zustand";

import { useWorkbenchStore } from "../app/store";

export interface AgentChatPlace {
  projectId: string;
  chatId: string;
}

/** Opens a chat in its project; resolves once the attempt settles. */
export type OpenAgentChat = (
  projectId: string,
  chatId: string,
) => Promise<void>;
/** The chat the Agent window shows now, if any. */
export type ShownAgentChat = () => AgentChatPlace | null;

export const MAX_AGENT_CHAT_PLACES = 50;

let back: AgentChatPlace[] = [];
let forward: AgentChatPlace[] = [];
let current: AgentChatPlace | null = null;
let travelling = false;

export const useAgentChatHistory = create<{
  canGoBack: boolean;
  canGoForward: boolean;
}>(() => ({ canGoBack: false, canGoForward: false }));

function publish() {
  const next = { canGoBack: back.length > 0, canGoForward: forward.length > 0 };
  const state = useAgentChatHistory.getState();
  if (
    state.canGoBack !== next.canGoBack ||
    state.canGoForward !== next.canGoForward
  ) {
    useAgentChatHistory.setState(next);
  }
}

const samePlace = (a: AgentChatPlace | null, b: AgentChatPlace | null) =>
  a?.projectId === b?.projectId && a?.chatId === b?.chatId;

function pushBounded(stack: AgentChatPlace[], place: AgentChatPlace) {
  stack.push(place);
  if (stack.length > MAX_AGENT_CHAT_PLACES) stack.shift();
}

/** Notes the chat the Agent window now shows. */
export function noteAgentChatVisit(place: AgentChatPlace) {
  // A trip may pass through other chats (a project opens on its own active
  // chat first); only where it lands counts.
  if (travelling || samePlace(current, place)) return;
  if (current) pushBounded(back, current);
  forward = [];
  current = { ...place };
  publish();
}

async function travel(
  from: AgentChatPlace[],
  to: AgentChatPlace[],
  open: OpenAgentChat,
  shown: ShownAgentChat,
) {
  if (travelling) return;
  const target = from.pop();
  if (!target) return;
  travelling = true;
  try {
    await open(target.projectId, target.chatId);
  } finally {
    travelling = false;
  }
  if (samePlace(shown(), target)) {
    if (current) pushBounded(to, current);
    current = target;
  }
  // Otherwise the chat is gone (archived, or its project removed): the
  // place is dropped and the window stays where it was.
  publish();
}

export function goBackInAgentChats(open: OpenAgentChat, shown: ShownAgentChat) {
  return travel(back, forward, open, shown);
}

export function goForwardInAgentChats(
  open: OpenAgentChat,
  shown: ShownAgentChat,
) {
  return travel(forward, back, open, shown);
}

const openInWorkbench: OpenAgentChat = (projectId, chatId) =>
  useWorkbenchStore.getState().openRailChat(projectId, chatId);

const shownInWorkbench: ShownAgentChat = () => {
  const { project, activeTabId } = useWorkbenchStore.getState();
  return project && activeTabId
    ? { projectId: project.id, chatId: activeTabId }
    : null;
};

/** Title-bar Back: reopens the chat shown before this one. */
export const goBackToPreviousChat = () =>
  goBackInAgentChats(openInWorkbench, shownInWorkbench);

/** Title-bar Forward: undoes a Back. */
export const goForwardToNextChat = () =>
  goForwardInAgentChats(openInWorkbench, shownInWorkbench);

/** For tests: forgets every place. */
export function resetAgentChatHistory() {
  back = [];
  forward = [];
  current = null;
  travelling = false;
  publish();
}

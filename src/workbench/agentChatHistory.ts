import { create } from "zustand";

import { useWorkbenchStore } from "../app/store";
import { currentShownAgentChatId } from "./shownAgentChat";

export interface AgentChatPlace {
  projectId: string;
  chatId: string;
}

export type AgentChatTrip =
  | { kind: "opened" }
  | { kind: "blocked" }
  | { kind: "missing" }
  | { kind: "landedElsewhere"; place: AgentChatPlace; targetGone: boolean };

export type OpenAgentChat = (place: AgentChatPlace) => Promise<AgentChatTrip>;

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

export function noteAgentChatVisit(place: AgentChatPlace) {
  // A trip may pass through other chats (a project opens on its own active
  // chat first); only where it ends counts.
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
) {
  const target = from.at(-1);
  if (travelling || !target) return;
  travelling = true;
  let trip: AgentChatTrip;
  try {
    trip = await open(target);
  } catch {
    trip = { kind: "blocked" };
  } finally {
    travelling = false;
  }
  switch (trip.kind) {
    case "blocked":
      return;
    case "missing":
      from.pop();
      break;
    case "opened":
      from.pop();
      if (current) pushBounded(to, current);
      current = target;
      break;
    case "landedElsewhere":
      if (trip.targetGone) from.pop();
      if (current) pushBounded(to, current);
      current = trip.place;
      break;
  }
  publish();
}

export function goBackInAgentChats(open: OpenAgentChat) {
  return travel(back, forward, open);
}

export function goForwardInAgentChats(open: OpenAgentChat) {
  return travel(forward, back, open);
}

function shownInWorkbench(): AgentChatPlace | null {
  const projectId = useWorkbenchStore.getState().project?.id;
  const chatId = currentShownAgentChatId();
  return projectId && chatId ? { projectId, chatId } : null;
}

const hasChat = (chatId: string) =>
  useWorkbenchStore
    .getState()
    .tabs.some((tab) => tab.id === chatId && tab.kind === "chat");

export const openInWorkbench: OpenAgentChat = async (target) => {
  const store = useWorkbenchStore.getState;
  if (store().projectTransitioning) return { kind: "blocked" };
  const switching = store().project?.id !== target.projectId;
  if (switching) {
    if (!store().projects.some((project) => project.id === target.projectId))
      return { kind: "missing" };
    await store().switchProject(target.projectId);
    if (store().project?.id !== target.projectId) return { kind: "blocked" };
  }
  const targetGone = !hasChat(target.chatId);
  if (targetGone && !switching) return { kind: "missing" };
  if (!targetGone) await store().activateTab(target.chatId);
  const shown = shownInWorkbench();
  if (samePlace(shown, target)) return { kind: "opened" };
  return switching && shown
    ? { kind: "landedElsewhere", place: shown, targetGone }
    : { kind: "blocked" };
};

export const goBackToPreviousChat = () => goBackInAgentChats(openInWorkbench);

export const goForwardToNextChat = () => goForwardInAgentChats(openInWorkbench);

export function resetAgentChatHistory() {
  back = [];
  forward = [];
  current = null;
  travelling = false;
  publish();
}

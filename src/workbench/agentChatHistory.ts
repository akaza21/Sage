// Back and Forward through the chats the Agent window has shown, as the
// Codex title bar offers. A chat is left behind when another one takes focus,
// in this project or another. History lives for the window session only.

import { create } from "zustand";

import { useWorkbenchStore } from "../app/store";
import { currentShownAgentChatId } from "./shownAgentChat";

export interface AgentChatPlace {
  projectId: string;
  chatId: string;
}

/** How a trip to a place in the history ended. */
export type AgentChatTrip =
  /** The window shows the place. */
  | { kind: "opened" }
  /** Nothing moved (a turn holds the project, or a transition is running). */
  | { kind: "blocked" }
  /** The place no longer exists and nothing moved. */
  | { kind: "missing" }
  /**
   * The project opened but not the chat, so the window shows `place`. When
   * the chat is gone its entry goes; otherwise it stays to try again.
   */
  | { kind: "landedElsewhere"; place: AgentChatPlace; targetGone: boolean };

/** Opens a place in its project and reports how far it got. */
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

/** Notes the chat the Agent window now shows. */
export function noteAgentChatVisit(place: AgentChatPlace) {
  // A trip may pass through other chats (a project opens on its own active
  // chat first); only where it ends counts.
  if (travelling || samePlace(current, place)) return;
  if (current) pushBounded(back, current);
  forward = [];
  current = { ...place };
  publish();
}

/** Moves from `from` towards `to`, keeping the history true to the window. */
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
      // The entry stays for when the window can move again.
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

/** The chat the Agent window shows, by the title bar's rule. */
function shownInWorkbench(): AgentChatPlace | null {
  const projectId = useWorkbenchStore.getState().project?.id;
  const chatId = currentShownAgentChatId();
  return projectId && chatId ? { projectId, chatId } : null;
}

const hasChat = (chatId: string) =>
  useWorkbenchStore
    .getState()
    .tabs.some((tab) => tab.id === chatId && tab.kind === "chat");

/** Opens a place through the same native paths as the rail. */
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

/** Title-bar Back: reopens the chat shown before this one. */
export const goBackToPreviousChat = () => goBackInAgentChats(openInWorkbench);

/** Title-bar Forward: undoes a Back. */
export const goForwardToNextChat = () => goForwardInAgentChats(openInWorkbench);

/** For tests: forgets every place. */
export function resetAgentChatHistory() {
  back = [];
  forward = [];
  current = null;
  travelling = false;
  publish();
}

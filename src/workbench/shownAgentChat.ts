// A side chat stands for its parent. The title bar and the chat history both
// read the shown chat through here, so they never disagree.

import { agentChatFocus, useAgentChatFocus } from "../app/agentChatOwner";
import { draftShownFor } from "../app/newChatDraftSlice";
import { useWorkbenchStore } from "../app/store";
import {
  railChatIdIn,
  useSideChatState,
  type SideChatEntry,
} from "./sideChatState";
import {
  collectChatTabIds,
  focusedAgentChatId,
  type AgentCenterScanTab,
} from "./workbenchWindowMode";

function focusedRailChatId(input: {
  tabs: ReadonlyArray<AgentCenterScanTab>;
  activeTabId: string | null;
  agentFocusId: string | null;
  sides: readonly SideChatEntry[];
}): string | null {
  return railChatIdIn(
    input.sides,
    focusedAgentChatId(
      collectChatTabIds(input.tabs),
      input.activeTabId,
      input.agentFocusId,
    ),
  );
}

export function currentShownAgentChatId(): string | null {
  const state = useWorkbenchStore.getState();
  const id = focusedRailChatId({
    tabs: state.tabs,
    activeTabId: state.activeTabId,
    agentFocusId: agentChatFocus(),
    sides: useSideChatState.getState().sides,
  });
  return draftShownFor(state, id) ? null : id;
}

function useFocusedRailChatId(): string | null {
  const tabs = useWorkbenchStore((state) => state.tabs);
  const activeTabId = useWorkbenchStore((state) => state.activeTabId);
  const agentFocusId = useAgentChatFocus();
  const sides = useSideChatState((state) => state.sides);
  return focusedRailChatId({ tabs, activeTabId, agentFocusId, sides });
}

export function useDraftShown(): boolean {
  const id = useFocusedRailChatId();
  return useWorkbenchStore((state) => draftShownFor(state, id));
}

/** The shown chat, kept current for a component. */
export function useShownAgentChatId(): string | null {
  const id = useFocusedRailChatId();
  const draftShown = useWorkbenchStore((state) => draftShownFor(state, id));
  return draftShown ? null : id;
}

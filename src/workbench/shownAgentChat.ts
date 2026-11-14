// The title bar and the chat history both read it through here, so they never
// disagree.

import { agentChatFocus, useAgentChatFocus } from "../app/agentChatOwner";
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

export function shownAgentChatId(input: {
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
  const { tabs, activeTabId } = useWorkbenchStore.getState();
  return shownAgentChatId({
    tabs,
    activeTabId,
    agentFocusId: agentChatFocus(),
    sides: useSideChatState.getState().sides,
  });
}

export function useShownAgentChatId(): string | null {
  const tabs = useWorkbenchStore((state) => state.tabs);
  const activeTabId = useWorkbenchStore((state) => state.activeTabId);
  const agentFocusId = useAgentChatFocus();
  const sides = useSideChatState((state) => state.sides);
  return shownAgentChatId({ tabs, activeTabId, agentFocusId, sides });
}

// Which chat the Agent window shows: the active chat tab, else the chat the
// Agent body kept focus on (while a file, diff or preview is active), else
// the first chat. A side chat stands for its parent. The title bar and the
// chat history both read it through here, so they never disagree.

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
  agentCenter?: "chat" | "draft";
  tabs: ReadonlyArray<AgentCenterScanTab>;
  activeTabId: string | null;
  agentFocusId: string | null;
  sides: readonly SideChatEntry[];
}): string | null {
  if (input.agentCenter === "draft") return null;
  return railChatIdIn(
    input.sides,
    focusedAgentChatId(
      collectChatTabIds(input.tabs),
      input.activeTabId,
      input.agentFocusId,
    ),
  );
}

/** The shown chat now, read outside React. */
export function currentShownAgentChatId(): string | null {
  const { tabs, activeTabId, agentCenter } = useWorkbenchStore.getState();
  return shownAgentChatId({
    tabs,
    agentCenter,
    activeTabId,
    agentFocusId: agentChatFocus(),
    sides: useSideChatState.getState().sides,
  });
}

/** The shown chat, kept current for a component. */
export function useShownAgentChatId(): string | null {
  const agentCenter = useWorkbenchStore((state) => state.agentCenter);
  const tabs = useWorkbenchStore((state) => state.tabs);
  const activeTabId = useWorkbenchStore((state) => state.activeTabId);
  const agentFocusId = useAgentChatFocus();
  const sides = useSideChatState((state) => state.sides);
  return shownAgentChatId({
    tabs,
    activeTabId,
    agentFocusId,
    sides,
    agentCenter,
  });
}

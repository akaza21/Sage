import type { AgentSessionChrome } from "../app/agentChatOwner";
import type { WorkbenchTab } from "../domain/workbench";
import type { ChatLiveSnapshot } from "../features/chat/acp/chatLiveSnapshot";
import { useAcpChatLiveSnapshot } from "../features/chat/acp/useAcpChatLiveSnapshot";
import { lazyParts } from "../features/chat/lazyParts";
import type * as SideHeadersChunk from "./AgentSideChatHeaders";
import { AgentTaskHeader } from "./AgentTaskHeader";
import { isSideChatId } from "./sideChatState";

const SideHeadersPart = lazyParts<typeof SideHeadersChunk>(
  () => import("./AgentSideChatHeaders"),
);

export interface AgentChatCenterHeaderProps {
  tabs: readonly WorkbenchTab[];
  paneIds: readonly string[] | null;
  focusedId: string | null;
  focusedSnapshot: ChatLiveSnapshot;
  chatCount: number;
  sessionChromeById: Readonly<Record<string, AgentSessionChrome>>;
  splitPercent: number;
  onArtifactHeaderMount?: (node: HTMLDivElement | null) => void;
  navigationCollapsed?: boolean;
}

export function AgentChatCenterHeader(props: AgentChatCenterHeaderProps) {
  const {
    tabs,
    paneIds,
    focusedId,
    focusedSnapshot,
    chatCount,
    sessionChromeById,
    splitPercent,
    onArtifactHeaderMount,
    navigationCollapsed,
  } = props;
  const split = Boolean(paneIds && paneIds.length > 1);
  const parentId = paneIds?.find((id) => !isSideChatId(id)) ?? null;
  const taskId = parentId ?? (!isSideChatId(focusedId) ? focusedId : null);
  const backgroundSnapshot = useAcpChatLiveSnapshot(
    taskId ?? "",
    Boolean(taskId && taskId !== focusedId),
  );
  return (
    <div
      className={`agent-chat-center__header${paneIds ? " agent-chat-center__header--panes" : ""}${split ? " agent-chat-center__header--split" : ""}`}
      style={
        split
          ? {
              gridTemplateColumns: `minmax(0, ${splitPercent}fr) minmax(0, ${
                100 - splitPercent
              }fr)`,
            }
          : undefined
      }
    >
      {taskId ? (
        <AgentTaskHeader
          key="task"
          tab={tabs.find((tab) => tab.id === taskId) ?? null}
          chatCount={chatCount}
          showNewChat={navigationCollapsed}
          snapshot={taskId === focusedId ? focusedSnapshot : backgroundSnapshot}
          sessionChrome={sessionChromeById[taskId]}
        />
      ) : null}
      {paneIds ? (
        <SideHeadersPart part="AgentSideChatHeaders" {...props} />
      ) : null}
      {onArtifactHeaderMount ? (
        <div
          key="artifact-slot"
          className="agent-artifact-header-slot"
          ref={onArtifactHeaderMount}
        />
      ) : null}
    </div>
  );
}

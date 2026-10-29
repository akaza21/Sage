import type { AgentSessionChrome } from "../app/agentChatOwner";
import type { WorkbenchTab } from "../domain/workbench";
import type { ChatLiveSnapshot } from "../features/chat/acp/chatLiveSnapshot";
import { lazyParts } from "../features/chat/lazyParts";
import type * as SideHeadersChunk from "./AgentSideChatHeaders";
import { AgentTaskHeader } from "./AgentTaskHeader";

const SideHeadersPart = lazyParts<typeof SideHeadersChunk>(
  () => import("./AgentSideChatHeaders"),
);

/** Name the focused Chat; side-chat panes each own a column-aligned header cell (s2/f0244). */
export interface AgentChatCenterHeaderProps {
  tabs: readonly WorkbenchTab[];
  /** Displayed panes if any is a side chat, else null. */
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
      {paneIds ? (
        <SideHeadersPart part="AgentSideChatHeaders" {...props} />
      ) : (
        <AgentTaskHeader
          key="task"
          tab={tabs.find((tab) => tab.id === focusedId) ?? null}
          chatCount={chatCount}
          showNewChat={navigationCollapsed}
          snapshot={focusedSnapshot}
          sessionChrome={sessionChromeById[focusedId ?? ""]}
        />
      )}
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

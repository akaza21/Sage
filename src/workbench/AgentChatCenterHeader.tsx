import type { AgentSessionChrome } from "../app/agentChatOwner";
import type { WorkbenchTab } from "../domain/workbench";
import type { ChatLiveSnapshot } from "../features/chat/acp/chatLiveSnapshot";
import { useAcpChatLiveSnapshot } from "../features/chat/acp/useAcpChatLiveSnapshot";
import { TranscriptPart } from "../features/chat/transcriptLazy";
import { AgentTaskHeader } from "./AgentTaskHeader";
import { isSideChatId } from "./sideChatState";

/** Name the focused Chat; side-chat panes each own a column-aligned header cell (s2/f0244). */
export function AgentChatCenterHeader({
  tabs,
  paneIds,
  focusedId,
  focusedSnapshot,
  chatCount,
  sessionChromeById,
  splitPercent,
  onArtifactHeaderMount,
}: {
  tabs: readonly WorkbenchTab[];
  /** Displayed panes if any is a side chat, else null. */
  paneIds: readonly string[] | null;
  focusedId: string | null;
  focusedSnapshot: ChatLiveSnapshot;
  chatCount: number;
  sessionChromeById: Readonly<Record<string, AgentSessionChrome>>;
  splitPercent: number;
  onArtifactHeaderMount?: (node: HTMLDivElement | null) => void;
}) {
  const ids = paneIds ?? (focusedId ? [focusedId] : []);
  const parentId = ids.find((id) => !isSideChatId(id)) ?? null;
  const parentSnapshot = useAcpChatLiveSnapshot(
    parentId ?? "",
    Boolean(parentId) && parentId !== focusedId,
  );
  const sideId = paneIds?.find(isSideChatId) ?? null;
  const sideSnapshot = useAcpChatLiveSnapshot(
    sideId ?? "",
    Boolean(sideId) && sideId !== focusedId,
  );
  const cells = paneIds ?? [focusedId];
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
      {cells.map((id) => {
        if (id && isSideChatId(id) && paneIds)
          return (
            <div key={id} className="agent-chat-center__side-tab">
              <TranscriptPart
                part="SideChatHeader"
                chatId={id}
                running={
                  (id === focusedId ? focusedSnapshot : sideSnapshot).working
                }
              />
            </div>
          );
        const tab = tabs.find((candidate) => candidate.id === id) ?? null;
        return (
          <AgentTaskHeader
            // Keep the task header mounted across Chat switches to restore trigger focus.
            key="task"
            tab={tab}
            chatCount={chatCount}
            snapshot={id === focusedId ? focusedSnapshot : parentSnapshot}
            // Parent status/actions remain its own while the side chat is focused.
            sessionChrome={sessionChromeById[id ?? ""]}
          />
        );
      })}
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

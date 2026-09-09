import type { RefObject } from "react";

import type { AcpChatPage } from "../../domain/acp";
import type { ChatMessage } from "../../domain/chat";
import type { ProjectRootSummary, ResourceRef } from "../../domain/workbench";
import { AcpChatProjection, type OpenResourceTarget } from "./acp";
import { ChatLegacyTranscript } from "./ChatLegacyTranscript";

export function ChatTranscript({
  transcriptRef,
  endRef,
  onScroll,
  projectTransitioning,
  useNativeAcp,
  page,
  focused,
  projectRoots,
  retentionIncomplete,
  onOpenResource,
  hasEarlier,
  loadingEarlier,
  historyError,
  onLoadEarlier,
  messages,
  developmentSimulator,
  followingEnd,
  onJumpToLatest,
}: {
  transcriptRef: RefObject<HTMLDivElement | null>;
  endRef: RefObject<HTMLDivElement | null>;
  onScroll: () => void;
  projectTransitioning: boolean;
  useNativeAcp: boolean;
  page: AcpChatPage | null;
  focused: boolean;
  projectRoots: readonly ProjectRootSummary[];
  retentionIncomplete: boolean;
  onOpenResource: (
    resource: ResourceRef,
    target: OpenResourceTarget,
    origin?: {
      projectId: AcpChatPage["projectId"];
      chatId: AcpChatPage["chatId"];
      sessionId: NonNullable<AcpChatPage["currentSessionId"]>;
    },
  ) => void;
  hasEarlier: boolean;
  loadingEarlier: boolean;
  historyError: string | null;
  onLoadEarlier: () => void;
  messages: readonly ChatMessage[];
  developmentSimulator: boolean;
  followingEnd: boolean;
  onJumpToLatest: () => void;
}) {
  return (
    <>
      <div ref={transcriptRef} className="message-list" onScroll={onScroll}>
        {projectTransitioning ? (
          <div className="composer__notice" role="status">
            Loading the authoritative project…
          </div>
        ) : useNativeAcp && page ? (
          <AcpChatProjection
            page={page}
            active
            focused={focused}
            projectRoots={projectRoots}
            retentionIncomplete={retentionIncomplete}
            onOpenResource={onOpenResource}
            hasEarlier={hasEarlier}
            loadingEarlier={loadingEarlier}
            historyError={historyError}
            onLoadEarlier={onLoadEarlier}
          />
        ) : (
          <ChatLegacyTranscript
            messages={messages}
            developmentSimulator={developmentSimulator}
          />
        )}
        <div ref={endRef} />
      </div>
      {!followingEnd ? (
        <button
          className="chat-follow-end"
          type="button"
          onClick={onJumpToLatest}
        >
          Jump to latest
        </button>
      ) : null}
    </>
  );
}

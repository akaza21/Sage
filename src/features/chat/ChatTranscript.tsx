import type { RefObject } from "react";

import type { AcpChatPage } from "../../domain/acp";
import type { ChatMessage } from "../../domain/chat";
import type { ProjectRootSummary, ResourceRef } from "../../domain/workbench";
import { AcpChatProjection, type OpenResourceTarget } from "./acp";
import { ChatLegacyTranscript } from "./ChatLegacyTranscript";
import { ChatTranscriptFooter } from "./ChatTranscriptFooter";
import "./chat-transcript-polish.css";

export function ChatTranscript({
  transcriptRef,
  contentRef,
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
  onRetry,
  showJumpToLatest,
  onJumpToLatest,
}: {
  transcriptRef: RefObject<HTMLDivElement | null>;
  contentRef: RefObject<HTMLDivElement | null>;
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
  onRetry?: () => void;
  showJumpToLatest: boolean;
  onJumpToLatest: () => void;
}) {
  return (
    <>
      <div ref={transcriptRef} className="message-list" onScroll={onScroll}>
        <div ref={contentRef} className="message-list__content">
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
          <ChatTranscriptFooter
            page={useNativeAcp ? page : null}
            messages={messages}
            onRetry={useNativeAcp ? onRetry : undefined}
          />
        </div>
        <div ref={endRef} />
      </div>
      {showJumpToLatest ? (
        <div className="chat-follow-end-zone">
          <button
            className="chat-follow-end"
            type="button"
            onClick={onJumpToLatest}
          >
            Jump to latest
          </button>
        </div>
      ) : null}
    </>
  );
}

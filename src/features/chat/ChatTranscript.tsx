import {
  useCallback,
  useLayoutEffect,
  useRef,
  useState,
  type RefObject,
} from "react";

import type { AcpChatPage } from "../../domain/acp";
import type { ChatMessage } from "../../domain/chat";
import type { ProjectRootSummary, ResourceRef } from "../../domain/workbench";
import {
  AcpChatProjection,
  latestRetryPrompt,
  type OpenResourceTarget,
} from "./acp";
import { ChatLegacyTranscript } from "./ChatLegacyTranscript";
import { ChatTranscriptFooter } from "./ChatTranscriptFooter";
import {
  AcpAnnouncementRegions,
  type Announcement,
} from "./acp/AcpAnnouncementRegions";
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
  const copySequence = useRef(0);
  const focusedRef = useRef(focused);
  useLayoutEffect(() => {
    focusedRef.current = focused;
  }, [focused]);
  const [copyFeedback, setCopyFeedback] = useState<{
    scope: string;
    announcement: Announcement;
  } | null>(null);
  const scope =
    useNativeAcp && page
      ? JSON.stringify([
          page.projectId,
          page.chatId,
          page.currentSessionId,
          page.turns.at(-1)?.id,
        ])
      : (messages.at(-1)?.text ?? "");
  const feedback =
    copyFeedback?.scope === scope ? copyFeedback.announcement : null;
  const announceCopy = (text: string) => {
    if (!focusedRef.current) return;
    setCopyFeedback({
      scope,
      announcement: { id: `answer-copy:${++copySequence.current}`, text },
    });
  };
  const forgetCopyFeedback = useCallback((id: string) => {
    setCopyFeedback((current) =>
      current?.announcement.id === id ? null : current,
    );
  }, []);
  const retryAction =
    useNativeAcp && page && latestRetryPrompt(page) ? onRetry : undefined;
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
              onRetry={retryAction}
              feedback={feedback}
              onFeedbackQueued={forgetCopyFeedback}
            />
          ) : (
            <ChatLegacyTranscript
              messages={messages}
              developmentSimulator={developmentSimulator}
            />
          )}
          <ChatTranscriptFooter
            key={scope}
            useNativeAcp={useNativeAcp}
            page={useNativeAcp ? page : null}
            messages={messages}
            onRetry={
              page?.turns.at(-1)?.state !== "failed" ? retryAction : undefined
            }
            onCopyFeedback={announceCopy}
          />
          {focused && (!useNativeAcp || !page) ? (
            <AcpAnnouncementRegions
              page={null}
              feedback={feedback}
              onFeedbackQueued={forgetCopyFeedback}
            />
          ) : null}
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

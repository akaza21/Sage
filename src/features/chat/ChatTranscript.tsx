import {
  useCallback,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type RefObject,
} from "react";

import type { AcpChatPage } from "../../domain/acp";
import { ArrowUp } from "../../ui/leanIcons";
import type { ChatMessage } from "../../domain/chat";
import type { ProjectRootSummary, ResourceRef } from "../../domain/workbench";
import {
  AcpChatProjection,
  latestRetryPrompt,
  type OpenResourceTarget,
} from "./acp";
import { ChatLegacyTranscript } from "./ChatLegacyTranscript";
import { ChatHome } from "./ChatHome";
import { composerWorkspaceName } from "./composerChrome";
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
  project = null,
  useNativeAcp,
  page,
  focused,
  projectRoots,
  retentionIncomplete,
  onOpenResource,
  hasEarlier,
  historyLoading = false,
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
  project?: { id: string; name: string } | null;
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
  historyLoading?: boolean;
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
  const [editSummaryContainer, setEditSummaryContainer] =
    useState<HTMLDivElement | null>(null);
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
  const retryAction = useMemo(
    () =>
      useNativeAcp && page && latestRetryPrompt(page) ? onRetry : undefined,
    [useNativeAcp, page, onRetry],
  );
  const projectName = composerWorkspaceName(project);
  const showHome = Boolean(
    useNativeAcp &&
    page?.projectId === project?.id &&
    page?.turns.length === 0 &&
    !projectTransitioning &&
    !loadingEarlier &&
    !historyError &&
    !hasEarlier &&
    page.nextCursor === null &&
    !retentionIncomplete,
  );
  const historyNotice = historyError ? (
    <div className="composer__notice" role="alert">
      Chat history could not be loaded: {historyError}
    </div>
  ) : (
    <div className="composer__notice" role="status">
      Loading chat history…
    </div>
  );
  return (
    <>
      <div
        ref={transcriptRef}
        className={`message-list${showHome ? " message-list--home" : ""}`}
        onScroll={onScroll}
      >
        <div ref={contentRef} className="message-list__content">
          {projectTransitioning ? (
            <div className="composer__notice" role="status">
              Loading the authoritative project…
            </div>
          ) : useNativeAcp && !page && (historyLoading || historyError) ? (
            historyNotice
          ) : useNativeAcp && page ? (
            <AcpChatProjection
              page={page}
              editSummaryContainer={editSummaryContainer}
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
              emptyState={
                showHome ? (
                  <ChatHome projectName={projectName} />
                ) : !hasEarlier &&
                  page.nextCursor === null &&
                  !retentionIncomplete &&
                  historyError ? (
                  historyNotice
                ) : undefined
              }
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
            aria-label="Jump to latest"
            title="Jump to latest"
            onClick={onJumpToLatest}
          >
            <ArrowUp size={16} aria-hidden="true" />
          </button>
        </div>
      ) : null}
      {useNativeAcp && !projectTransitioning ? (
        <div className="chat-edit-summary-zone" ref={setEditSummaryContainer} />
      ) : null}
    </>
  );
}

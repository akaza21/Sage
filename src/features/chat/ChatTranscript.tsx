import {
  useCallback,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type RefObject,
} from "react";
import { type AcpChatPage } from "../../domain/acp";
import { type ChatMessage } from "../../domain/chat";
import {
  type ProjectRootSummary,
  type ResourceRef,
} from "../../domain/workbench";
import { ArrowDown } from "../../ui/leanIcons";
import { isSideChatId } from "../../workbench/sideChatState";
import { ChatHome } from "./ChatHome";
import { ChatLegacyTranscript } from "./ChatLegacyTranscript";
import { ChatTranscriptFooter } from "./ChatTranscriptFooter";
import {
  AcpChatProjection,
  latestRetryPrompt,
  type OpenResourceTarget,
} from "./acp";
import {
  AcpAnnouncementRegions,
  type Announcement,
} from "./acp/AcpAnnouncementRegions";
import { AnswerCopyFeedback } from "./answerCopyFeedback";
import "./chat-home.css";
import "./chat-transcript-polish.css";
import { composerWorkspaceName } from "./composerChrome";
import { ForkPart, NoticePart } from "./lazyParts";
import { isNewProjectChat } from "./newProjectChat";
import { TranscriptInteractionsGate, TranscriptPart } from "./transcriptLazy";

export function ChatTranscript({
  editSummaryContainer = null,
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
  editSummaryContainer?: HTMLDivElement | null;
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
  /** "activity": follow is paused and a running turn grew below. */
  showJumpToLatest: boolean | "activity";
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
  // Stable per scope: it is also every earlier and inherited answer's
  // Copy announcer (AnswerCopyFeedback).
  const announceCopy = useCallback(
    (text: string) => {
      if (!focusedRef.current) return;
      setCopyFeedback({
        scope,
        announcement: { id: `answer-copy:${++copySequence.current}`, text },
      });
    },
    [scope],
  );
  const forgetCopyFeedback = useCallback((id: string) => {
    setCopyFeedback((current) =>
      current?.announcement.id === id ? null : current,
    );
  }, []);
  const retryAction = useMemo(
    () =>
      // A side chat is temporary: nothing in it offers Retry.
      useNativeAcp &&
      page &&
      !isSideChatId(page.chatId) &&
      latestRetryPrompt(page)
        ? onRetry
        : undefined,
    [useNativeAcp, page, onRetry],
  );
  const projectName = composerWorkspaceName(project, 1024);
  const jumpLabel =
    showJumpToLatest === "activity"
      ? "New activity below. Jump to latest"
      : "Jump to latest";
  const forkedFrom = useNativeAcp ? page?.forkedFrom : undefined;
  const showHome =
    !forkedFrom &&
    isNewProjectChat({
      useNativeAcp,
      page,
      project,
      projectTransitioning,
      loadingEarlier,
      historyError,
      hasEarlier,
      retentionIncomplete,
    });
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
    <AnswerCopyFeedback.Provider value={announceCopy}>
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
            <>
              {forkedFrom && !hasEarlier ? (
                <ForkPart
                  part="ForkDivider"
                  parentChatId={forkedFrom}
                  forkTurnId={page.forkedFromTurnId}
                />
              ) : null}
              {page.sessionNotices?.length ? (
                <NoticePart
                  part="SessionNoticeStrip"
                  session={page.chatId + page.currentSessionId}
                  notices={page.sessionNotices}
                />
              ) : null}
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
                    isSideChatId(page.chatId) ? (
                      <TranscriptPart part="SideChatHome" />
                    ) : (
                      <ChatHome projectName={projectName} />
                    )
                  ) : !hasEarlier &&
                    page.nextCursor === null &&
                    !retentionIncomplete &&
                    historyError ? (
                    historyNotice
                  ) : undefined
                }
              />
            </>
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
        {useNativeAcp && page && !projectTransitioning ? (
          <TranscriptInteractionsGate
            key={page.chatId}
            chatId={page.chatId}
            focused={focused}
            contentRef={contentRef}
          />
        ) : null}
      </div>

      <button
        className="chat-follow-end"
        type="button"
        hidden={!showJumpToLatest}

        data-mode={showJumpToLatest === "activity" ? "activity" : undefined}
        aria-label={jumpLabel}
        title={jumpLabel}
        onClick={onJumpToLatest}
      >
        <ArrowDown size={24} aria-hidden="true" />
      </button>
    </AnswerCopyFeedback.Provider>
  );
}

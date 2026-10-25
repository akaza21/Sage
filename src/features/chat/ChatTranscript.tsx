import {
  useCallback,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type RefObject,
} from "react";

import type { AcpChatPage } from "../../domain/acp";
import { ArrowDown, Folder } from "../../ui/leanIcons";
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
import { isNewProjectChat } from "./newProjectChat";
import { ChatTranscriptFooter } from "./ChatTranscriptFooter";
import { AnswerCopyFeedback } from "./answerCopyFeedback";
import {
  AcpAnnouncementRegions,
  type Announcement,
} from "./acp/AcpAnnouncementRegions";
import "./chat-transcript-polish.css";
import { isSideChatId } from "../../workbench/sideChatState";
import { TranscriptInteractionsGate, TranscriptPart } from "./transcriptLazy";
import { ForkPart } from "./lazyParts";

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
  /** "activity": follow is paused and a running turn grew below (Codex s6/f0200). */
  showJumpToLatest: boolean | "activity";
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
      // A side chat is temporary: nothing in it offers Retry (Codex s2/f0352).
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
      {/* It overlays the transcript and stays mounted while hidden so CSS
          can fade it out (Codex s2/f0048). */}
      <button
        className="chat-follow-end"
        type="button"
        hidden={!showJumpToLatest}
        // data-mode="activity" restyles the arrow into the middle of three
        // dots (chat-follow-end.css, Codex s6/f0200).
        data-mode={showJumpToLatest === "activity" ? "activity" : undefined}
        aria-label={jumpLabel}
        title={jumpLabel}
        onClick={onJumpToLatest}
      >
        <ArrowDown size={14} aria-hidden="true" />
      </button>
      {useNativeAcp && !projectTransitioning ? (
        <div className="chat-edit-summary-zone" ref={setEditSummaryContainer}>
          {/* A side chat inherits its parent's Project (Codex s2/f0244). */}
          {showHome && projectName && !isSideChatId(page?.chatId) ? (
            <div
              className="chat-home__scope"
              role="group"
              aria-label="Project scope"
            >
              <Folder size={14} aria-hidden="true" />
              <span>{projectName}</span>
            </div>
          ) : null}
        </div>
      ) : null}
    </AnswerCopyFeedback.Provider>
  );
}

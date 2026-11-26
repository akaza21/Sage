import { Fragment, memo, useMemo, useRef } from "react";
import { createPortal } from "react-dom";

import type { AcpChatPage, AcpItem, TurnProjection } from "../../../domain/acp";
import type { AgentArtifactOwner } from "../../../domain/agentArtifact";
import type { ResourceRef } from "../../../domain/workbench";
import { isTauriRuntime } from "../../../ports/desktopRuntime";
import { acpDesktop } from "../../../services/acpDesktop";
import { RotateCcw } from "../../../ui/leanIcons";
import { isSideChatId } from "../../../workbench/sideChatState";
import { AnswerActionButton, AnswerActions } from "../AnswerActions";
import type { TurnListHost } from "../chatScroll";
import { ChatTranscriptTurnSummary } from "../ChatTranscriptFooter";
import { turnHasActivities } from "../transcriptSummary";
import { EditSummaryPart, NavigationPart, TranscriptPart } from "../lazyParts";
import { TurnDaySeparator } from "./AcpTurnDates";
import {
  chronologicalTurnItems,
  promptAttachmentItems,
  turnBindingId,
  currentSession,
} from "./acpTurnState";
import { projectRootLabelKey, projectRootLabels } from "./agentEditSummary";
import "./browser-operations.css";
import { BrowserOperations } from "./BrowserOperations";
import { DeferredTurnFileActivity as TurnFileActivity } from "./DeferredChatDetails";
import { isEditingFiles } from "./editingFiles";
import { RootHarnessActivities } from "./HarnessActivity";
import { ImageItem } from "./ImageItem";
import { PrimaryActivities } from "./PrimaryActivities";
import { terminalTurnStates, turnReasonErrorItem } from "./statusSemantics";
import { type OpenResourceTarget } from "./ToolItem";
import { ToolRunningIndicator } from "./ToolRunningIndicator";
import { ItemView } from "./TurnActivityItem";
import { RetryTurnButton, TurnFailure } from "./TurnFailure";
import { TurnHeader } from "./TurnHeader";
import { PromptPageContext, renderTurnPrompts } from "./TurnPrompts";
import { TurnSessionActivities } from "./TurnSessionActivities";
import { useTurnWorkspaceChanges } from "./turnWorkspaceBridge";
import type { TurnWorkspaceChange } from "./turnWorkspaceChanges";
import { useTurnDayTimestamps } from "./useTurnDayTimestamps";
import { VirtualTurnList } from "./VirtualTurnList";

import type { AcpChatPresentationProps } from "./acpChatPresentationTypes";
export type { AcpChatPresentationProps } from "./acpChatPresentationTypes";

export const Turn = memo(
  function Turn({
    page,
    turn,
    rootLabels,
    workspaceChanges,
    workspaceIncomplete = false,
    onOpenResource,
    onRetry,
    inherited = false,
    latest = false,
    fileSummaryInDock = false,
  }: {
    page: AcpChatPage;
    turn: TurnProjection;
    inherited?: boolean;
    latest?: boolean;
    fileSummaryInDock?: boolean;
    rootLabels?: Readonly<Record<string, string>>;
    workspaceChanges?: readonly TurnWorkspaceChange[];
    workspaceIncomplete?: boolean;
    onOpenResource?: AcpChatPresentationProps["onOpenResource"];
    onRetry?: () => void;
  }) {
    const primary = turn.items.filter(
      (item) => item.origin === "primary" && item.kind !== "fileChangeDiff",
    );
    const interactive = !inherited && turn.sessionId === page.currentSessionId;
    const failurePage = latest && interactive ? page : undefined;
    const fileReferenceOwner: AgentArtifactOwner | undefined =
      rootLabels && page.projectId && page.chatId && page.currentSessionId
        ? {
            projectId: page.projectId,
            chatId: page.chatId,
            sessionId: page.currentSessionId,
          }
        : undefined;
    const openForTurn = useMemo(() => {
      if (!interactive || !onOpenResource) return undefined;
      const actionOrigin = {
        projectId: page.projectId,
        chatId: page.chatId,
        sessionId: turn.sessionId,
      };
      return (resource: ResourceRef, target: OpenResourceTarget = "file") =>
        onOpenResource(resource, target, actionOrigin);
    }, [
      interactive,
      onOpenResource,
      page.chatId,
      page.projectId,
      turn.sessionId,
    ]);
    const terminal = terminalTurnStates.has(turn.state);
    const stopped = turn.state === "cancelled" || turn.state === "interrupted";
    const promptItems = primary.filter(
      (item) => item.kind === "text" && item.payload.role === "user",
    );
    const attachmentItems = promptAttachmentItems(primary);
    const attachmentIds = new Set(attachmentItems.map((item) => item.id));
    const chronologicalItems = chronologicalTurnItems(
      primary,
      attachmentIds,
      terminal,
    );
    const newestItems = [...chronologicalItems].reverse();
    const finalAnswer = terminal
      ? newestItems.find(
          (item): item is Extract<AcpItem, { kind: "text" }> =>
            item.kind === "text" && item.payload.role === "assistant",
        )
      : undefined;
    const liveThought = newestItems.find(
      (item) => item.kind === "thoughtSummary",
    );
    const failureItem =
      turn.state === "failed" ? turnReasonErrorItem(turn) : undefined;
    const workingItems = chronologicalItems.filter(
      (item) =>
        item.id !== failureItem?.id &&
        item.id !== finalAnswer?.id &&
        (item.kind !== "thoughtSummary" ||
          (!terminal && item.id === liveThought?.id)),
    );
    const text = finalAnswer?.payload.text;
    // Rust forks only the latest Turn.
    const actions = text ? (
      <AnswerActions
        text={text}
        createdAt={finalAnswer.createdAt}
        page={page}
        forkable={latest}
      >
        {onRetry && turn.state !== "failed" ? (
          <AnswerActionButton label="Retry" onClick={onRetry}>
            <RotateCcw size={14} aria-hidden="true" />
          </AnswerActionButton>
        ) : null}
      </AnswerActions>
    ) : null;
    const renderWorkingItem = (item: AcpItem) => (
      <ItemView
        key={item.id}
        item={item}
        workingItems={turn.items}
        interactive={interactive}
        fileReferenceOwner={fileReferenceOwner}
        onOpenResource={openForTurn}
      />
    );
    const content = (
      <>
        <section
          className="acp-turn"
          inert={inherited}
          data-turn-state={turn.state}
          aria-label={inherited ? undefined : `Turn ${turn.ordinal}`}
          data-turn-id={turn.id}
          data-session-id={turn.sessionId}
          data-runtime-epoch={turn.runtimeEpoch}
          data-turn-ordinal={turn.ordinal}
        >
          {!stopped ? <TurnHeader turn={turn} /> : null}
          <div className="acp-turn__items">
            {turn.attachedContext ? (
              <TranscriptPart
                part="SentAnnotations"
                contexts={turn.attachedContext}
                chatId={page.chatId}
              />
            ) : null}
            {attachmentItems.length > 0 ? (
              <div
                className="acp-turn__attachments"
                aria-label="Prompt attachments"
              >
                {attachmentItems.map((item) => (
                  <ImageItem key={item.id} item={item} thumbnail />
                ))}
              </div>
            ) : null}
            {renderTurnPrompts(promptItems, turn, interactive)}
            <ChatTranscriptTurnSummary
              turn={turn}
              runningHeading={!isSideChatId(page.chatId)}
              steered={workingItems
                .filter((item) => item.kind === "userPrompt")
                .map(renderWorkingItem)}
              hasActivities={turnHasActivities(turn, workingItems.length)}
            >
              {workingItems.length > 0 ? (
                <PrimaryActivities
                  items={workingItems}
                  running={
                    turn.state === "running" || turn.state === "waitingModel"
                  }
                  interactive={interactive}
                  onOpenResource={openForTurn}
                  renderItem={renderWorkingItem}
                />
              ) : null}
              <TurnSessionActivities activities={turn.sessionActivities} />
              <BrowserOperations
                operations={turn.browserOperations ?? []}
                collapsed={turn.browserOperationsCollapsed ?? 0}
              />
            </ChatTranscriptTurnSummary>
            {finalAnswer ? (
              <ItemView
                key={finalAnswer.id}
                item={finalAnswer}
                selectableTurnId={inherited ? undefined : turn.id}
                turnState={turn.state}
                interactive={interactive}
                fileReferenceOwner={fileReferenceOwner}
              />
            ) : null}
            <RootHarnessActivities
              turn={turn}
              renderItem={(item) =>
                item.id === failureItem?.id ? null : (
                  <ItemView
                    item={item}
                    turnState={turn.state}
                    interactive={interactive}
                    fileReferenceOwner={fileReferenceOwner}
                    onOpenResource={openForTurn}
                  />
                )
              }
            />
            {isEditingFiles(page, turn) &&
            !turn.items.some(
              (item) =>
                item.kind === "toolCall" &&
                item.toolState === "inProgress" &&
                item.payload.presentation.kind === "edit",
            ) ? (
              <div className="acp-editing-files acp-tool-rollup__summary-copy">
                <ToolRunningIndicator />
                <span className="acp-live-shimmer">Editing files</span>
              </div>
            ) : null}
            {terminal && fileSummaryInDock ? (
              workspaceIncomplete ? (
                <NavigationPart part="WorkspaceTrackingNotice" />
              ) : null
            ) : terminal ? (
              <TurnFileActivity
                page={page}
                turn={turn}
                workspaceChanges={workspaceChanges}
                workspaceIncomplete={workspaceIncomplete}
                rootLabels={rootLabels}
                onOpenResource={interactive ? onOpenResource : undefined}
              />
            ) : null}
            {stopped ? <TurnHeader turn={turn} /> : null}
            {stopped && !finalAnswer && onRetry ? (
              <RetryTurnButton onRetry={onRetry} />
            ) : null}
            <TurnFailure turn={turn} onRetry={onRetry} page={failurePage} />
          </div>
          {!inherited && actions}
        </section>
        {inherited && actions}
      </>
    );
    return content;
  },
  (left, right) =>
    left.turn === right.turn &&
    currentSession(left.page)?.runtimeEpoch ===
      currentSession(right.page)?.runtimeEpoch &&
    left.page.projectId === right.page.projectId &&
    left.page.chatId === right.page.chatId &&
    left.page.currentSessionId === right.page.currentSessionId &&
    // `inherited` is fixed per mount: only InheritedTurns passes it.
    left.rootLabels === right.rootLabels &&
    left.workspaceChanges === right.workspaceChanges &&
    left.workspaceIncomplete === right.workspaceIncomplete &&
    left.latest === right.latest &&
    turnBindingId(left.page, left.turn) ===
      turnBindingId(right.page, right.turn) &&
    left.fileSummaryInDock === right.fileSummaryInDock &&
    left.onOpenResource === right.onOpenResource &&
    left.onRetry === right.onRetry,
);

export function AcpChatPresentation({
  page,
  onOpenResource,
  hasEarlier,
  loadingEarlier,
  historyError,
  onLoadEarlier,
  projectRoots,
  retentionIncomplete,
  onRetry,
  onReviewChanges,
  editSummaryContainer,
}: AcpChatPresentationProps) {
  const { changes: workspaceChanges, incompleteTurnIds } =
    useTurnWorkspaceChanges(page, isTauriRuntime());
  const navigationRef = useRef<TurnListHost | null>(null);
  const countIncomplete = page.nextCursor !== null || retentionIncomplete;
  const dayTimestamps = useTurnDayTimestamps(page.turns);
  const labelsKey = projectRootLabelKey(projectRoots);
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const labels = useMemo(() => projectRootLabels(projectRoots), [labelsKey]);
  const editSummary = (
    <EditSummaryPart
      part="TurnEditSummary"
      onOpenResource={onOpenResource}
      key={page.projectId + ":" + page.chatId + ":" + page.currentSessionId}
      page={page}
      rootLabels={labels}
      countIncomplete={countIncomplete}
      workspaceChanges={workspaceChanges}
      workspaceIncomplete={incompleteTurnIds.size > 0}
      onRecordDisposition={acpDesktop.recordAgentEditDisposition}
      onReview={onReviewChanges}
    />
  );
  const renderTurn = (turn: TurnProjection, index: number) => (
    <Fragment key={turn.id}>
      <TurnDaySeparator timestamp={dayTimestamps[index]} />
      <Turn
        page={page}
        turn={turn}
        rootLabels={projectRoots.length > 0 ? labels : undefined}
        workspaceChanges={workspaceChanges}
        workspaceIncomplete={incompleteTurnIds.has(turn.id)}
        onOpenResource={onOpenResource}
        onRetry={index === page.turns.length - 1 ? onRetry : undefined}
        latest={index === page.turns.length - 1}
        fileSummaryInDock={editSummaryContainer !== undefined}
      />
    </Fragment>
  );
  return (
    <>
      <NavigationPart
        part="ChatNavigation"
        // Use a distinct sibling key to avoid remounting and closing find.
        key={`navigation:${page.projectId}:${page.chatId}`}
        turns={page.turns}
        chatId={page.chatId}
        hostRef={navigationRef}
        incomplete={hasEarlier || countIncomplete}
      />
      {hasEarlier || historyError || countIncomplete ? (
        <NavigationPart
          part="AcpHistoryNotices"
          scope={JSON.stringify([page.chatId, page.currentSessionId])}
          hasEarlier={hasEarlier}
          loadingEarlier={loadingEarlier}
          historyError={historyError}
          onLoadEarlier={onLoadEarlier}
          unloaded={page.nextCursor !== null}
          retentionIncomplete={retentionIncomplete}
        />
      ) : null}
      <PromptPageContext value={page}>
        {page.turns.length > 40 ? (
          <VirtualTurnList
            key={`turns:${page.projectId}:${page.chatId}`}
            turns={page.turns}
            renderTurn={renderTurn}
            navigationRef={navigationRef}
          />
        ) : (
          page.turns.map(renderTurn)
        )}
      </PromptPageContext>
      {editSummaryContainer
        ? createPortal(editSummary, editSummaryContainer)
        : null}
    </>
  );
}

export default AcpChatPresentation;

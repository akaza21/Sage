import { Fragment, memo, useCallback, useMemo, useRef } from "react";
import { createPortal } from "react-dom";

import type { AcpChatPage, TurnProjection } from "../../../domain/acp";
import type { AgentArtifactOwner } from "../../../domain/agentArtifact";
import type { AgentDiffRef } from "../../../domain/agentDiff";
import type { ResourceRef } from "../../../domain/workbench";
import { isTauriRuntime } from "../../../ports/desktopRuntime";
import { acpDesktop } from "../../../services/acpDesktop";
import { AnswerActions } from "../AnswerActions";
import type { TurnListHost } from "../chatScroll";
import { ChatTranscriptTurnSummary } from "../ChatTranscriptFooter";
import { EditSummaryPart, NavigationPart, TranscriptPart } from "../lazyParts";
import { TurnDaySeparator } from "./AcpTurnDates";
import {
  chronologicalTurnItems,
  promptAttachmentItems,
  rootActivities,
} from "./acpTurnState";
import { projectRootLabelKey, projectRootLabels } from "./agentEditSummary";
import "./browser-operations.css";
import { BrowserOperations } from "./BrowserOperations";
import { DeferredTurnFileActivity as TurnFileActivity } from "./DeferredChatDetails";
import { isEditingFiles } from "./editingFiles";
import { HarnessActivity } from "./HarnessActivity";
import { PrimaryActivities } from "./PrimaryActivities";
import { terminalTurnStates } from "./statusSemantics";
import { type OpenResourceTarget } from "./ToolItem";
import { ToolRunningIndicator } from "./ToolRunningIndicator";
import { ItemView } from "./TurnActivityItem";
import { TurnFailure } from "./TurnFailure";
import { TurnHeader } from "./TurnHeader";
import { useTurnWorkspaceChanges } from "./turnWorkspaceBridge";
import type { TurnWorkspaceChange } from "./turnWorkspaceChanges";
import { useTurnDayTimestamps } from "./useTurnDayTimestamps";
import { VirtualTurnList } from "./VirtualTurnList";

import type { AcpChatPresentationProps } from "./acpChatPresentationTypes";
export type { AcpChatPresentationProps } from "./acpChatPresentationTypes";

const Turn = memo(
  function Turn({
    page,
    turn,
    rootLabels,
    workspaceChanges,
    workspaceIncomplete = false,
    onOpenResource,
    onRetry,
    inherited = false,
    answerActions = inherited,
  }: {
    page: AcpChatPage;
    turn: TurnProjection;
    /** A fork's read-only parent Turn: never interactive or quotable. */
    inherited?: boolean;
    /** Earlier/inherited answers retain Copy. */
    answerActions?: boolean;
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
    const finalAnswer = terminal
      ? [...chronologicalItems]
          .reverse()
          .find(
            (item) => item.kind === "text" && item.payload.role === "assistant",
          )
      : undefined;
    const liveThought = [...chronologicalItems]
      .reverse()
      .find((item) => item.kind === "thoughtSummary");
    const workingItems = chronologicalItems.filter(
      (item) =>
        item.id !== finalAnswer?.id &&
        (item.kind !== "thoughtSummary" ||
          (!terminal && item.id === liveThought?.id)),
    );
    // Earlier/inherited answer menus never fork: Rust forks only the latest Turn (footer).
    const text = finalAnswer?.kind === "text" && finalAnswer.payload.text;
    const actions =
      answerActions && text ? (
        <AnswerActions
          text={text}
          createdAt={finalAnswer.createdAt}
          page={page}
        />
      ) : null;
    // An inherited Turn is inert, all but its answer's own row.
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
                  <ItemView
                    key={item.id}
                    item={item}
                    interactive={interactive}
                  />
                ))}
              </div>
            ) : null}
            {promptItems.map((item) => (
              <ItemView key={item.id} item={item} interactive={interactive} />
            ))}
            <ChatTranscriptTurnSummary
              turn={turn}
              hasActivities={
                workingItems.length > 0 ||
                (turn.browserOperations?.length ?? 0) > 0 ||
                (turn.browserOperationsCollapsed ?? 0) > 0
              }
            >
              {workingItems.length > 0 ? (
                <PrimaryActivities
                  items={workingItems}
                  running={
                    turn.state === "running" || turn.state === "waitingModel"
                  }
                  interactive={interactive}
                  onOpenResource={openForTurn}
                  renderItem={(item) => (
                    <ItemView
                      key={item.id}
                      item={item}
                      workingItems={turn.items}
                      interactive={interactive}
                      fileReferenceOwner={fileReferenceOwner}
                      onOpenResource={openForTurn}
                    />
                  )}
                />
              ) : null}
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
            {!inherited && actions}
            {rootActivities(turn).map((activity) => (
              <HarnessActivity
                key={activity.id}
                activity={activity}
                activities={turn.activities}
                items={turn.items}
                renderItem={(item) => (
                  <ItemView
                    item={item}
                    turnState={turn.state}
                    interactive={interactive}
                    fileReferenceOwner={fileReferenceOwner}
                    onOpenResource={openForTurn}
                  />
                )}
              />
            ))}
            {/* Live edits precede file data. */}
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
            {terminal ? (
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
              <button type="button" onClick={onRetry}>
                Retry
              </button>
            ) : null}
            <TurnFailure turn={turn} onRetry={onRetry} />
          </div>
        </section>
        {inherited && actions}
      </>
    );
    if (!inherited) return content;
    return (
      <section className="chat-fork-turn" aria-label={`Turn ${turn.ordinal}`}>
        {content}
      </section>
    );
  },
  (left, right) =>
    left.turn === right.turn &&
    left.page.sessions.some((session) => session.id === left.turn.sessionId) ===
      right.page.sessions.some(
        (session) => session.id === right.turn.sessionId,
      ) &&
    left.page.projectId === right.page.projectId &&
    left.page.chatId === right.page.chatId &&
    left.page.currentSessionId === right.page.currentSessionId &&
    // `inherited` is fixed per mount: only InheritedTurns passes it.
    left.rootLabels === right.rootLabels &&
    left.workspaceChanges === right.workspaceChanges &&
    left.workspaceIncomplete === right.workspaceIncomplete &&
    left.answerActions === right.answerActions &&
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
  const onOpenDiff = useCallback(
    (resource: ResourceRef, diffRef: AgentDiffRef) =>
      onOpenResource?.(
        resource,
        "diff",
        {
          projectId: page.projectId,
          chatId: page.chatId,
          sessionId: diffRef.sessionId,
        },
        diffRef,
      ),
    [onOpenResource, page.projectId, page.chatId],
  );
  const onOpenFile = useCallback(
    (resource: ResourceRef, turn: TurnProjection) =>
      onOpenResource?.(resource, "file", {
        projectId: page.projectId,
        chatId: page.chatId,
        sessionId: turn.sessionId,
      }),
    [onOpenResource, page.projectId, page.chatId],
  );
  const editSummary = (
    <EditSummaryPart
      part="TurnEditSummary"
      key={page.projectId + ":" + page.chatId + ":" + page.currentSessionId}
      page={page}
      rootLabels={labels}
      countIncomplete={countIncomplete}
      workspaceChanges={workspaceChanges}
      workspaceIncomplete={incompleteTurnIds.size > 0}
      onOpenDiff={onOpenResource ? onOpenDiff : undefined}
      onOpenFile={onOpenResource ? onOpenFile : undefined}
      onRecordDisposition={acpDesktop.recordAgentEditDisposition}
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
        answerActions={index < page.turns.length - 1}
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
        // History notices stay in the navigation chunk.
        <NavigationPart
          part="AcpHistoryNotices"
          hasEarlier={hasEarlier}
          loadingEarlier={loadingEarlier}
          historyError={historyError}
          onLoadEarlier={onLoadEarlier}
          unloaded={page.nextCursor !== null}
          retentionIncomplete={retentionIncomplete}
        />
      ) : null}
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
      {editSummaryContainer
        ? createPortal(editSummary, editSummaryContainer)
        : null}
    </>
  );
}

/** Inherited Turns mount no navigator/find/virtual list/edit summary; the fork transcript owns those. */
export function InheritedTurns({ page }: { page: AcpChatPage }) {
  const dayTimestamps = useTurnDayTimestamps(page.turns);
  return page.turns.map((turn, index) => (
    <Fragment key={turn.id}>
      <TurnDaySeparator timestamp={dayTimestamps[index]} />
      <Turn page={page} turn={turn} inherited />
    </Fragment>
  ));
}

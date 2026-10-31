import { usePresentedAttention } from "../../../app/chatAttentionPresentation";
import { Fragment, useCallback, useMemo, useRef } from "react";
import { createPortal } from "react-dom";
import type { AcpChatPage, TurnProjection } from "../../../domain/acp";
import type { AgentDiffRef } from "../../../domain/agentDiff";
import type { ResourceRef } from "../../../domain/workbench";
import { acpDesktop } from "../../../services/acpDesktop";
import { TurnDaySeparator } from "./AcpTurnDates";
import { useTurnDayTimestamps } from "./useTurnDayTimestamps";
import { projectRootLabelKey, projectRootLabels } from "./agentEditSummary";
import { useTurnWorkspaceChanges } from "./turnWorkspaceBridge";
import { isTauriRuntime } from "../../../ports/desktopRuntime";
import { VirtualTurnList } from "./VirtualTurnList";
import type { TurnListHost } from "../chatScroll";
import { EditSummaryPart, NavigationPart } from "../lazyParts";
import { AcpTranscriptTurn as Turn } from "./AcpTranscriptTurn";
import "./browser-operations.css";
import type { AcpChatPresentationProps } from "./acpChatPresentationTypes";
export type { AcpChatPresentationProps } from "./acpChatPresentationTypes";

export function AcpChatPresentation({
  page,
  focused = false,
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
  usePresentedAttention(page, focused);
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

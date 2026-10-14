import { Fragment, memo, useCallback, useMemo } from "react";
import { createPortal } from "react-dom";
import { FilePenLine } from "../../../ui/leanIcons";

import type { AcpChatPage, TurnProjection } from "../../../domain/acp";
import type {
  AgentArtifactOpenHandler,
  AgentArtifactOwner,
} from "../../../domain/agentArtifact";
import type { AgentDiffRef } from "../../../domain/agentDiff";
import type {
  ProjectRootSummary,
  ResourceRef,
} from "../../../domain/workbench";
import { acpDesktop } from "../../../services/acpDesktop";
import { ChatTranscriptTurnSummary } from "../ChatTranscriptFooter";
import { TurnDaySeparator } from "./AcpTurnDates";
import { useTurnDayTimestamps } from "./useTurnDayTimestamps";
import {
  itemOwnershipMatches,
  projectRootLabelKey,
  projectRootLabels,
} from "./agentEditSummary";
import { TurnEditSummary } from "./TurnEditSummary";
import { HarnessActivity } from "./HarnessActivity";
import { promptAttachmentItems } from "./acpTurnState";
import type { OpenResourceTarget } from "./ToolItem";
import { PrimaryActivities } from "./PrimaryActivities";
import { ItemView } from "./TurnActivityItem";
import { TurnHeader } from "./TurnHeader";
import { TurnFailure } from "./TurnFailure";
import { BrowserOperations } from "./BrowserOperations";
import { terminalItemLifecycles, terminalTurnStates } from "./statusSemantics";
import { VirtualTurnList } from "./VirtualTurnList";
import "./browser-operations.css";

export type AcpChatPresentationProps = {
  page: AcpChatPage;
  onOpenResource?: AgentArtifactOpenHandler;
  hasEarlier: boolean;
  loadingEarlier: boolean;
  historyError: string | null;
  onLoadEarlier?: () => void;
  projectRoots: readonly Pick<ProjectRootSummary, "id" | "name" | "ordinal">[];
  retentionIncomplete: boolean;
  onRetry?: () => void;
  editSummaryContainer?: HTMLElement | null;
};

const Turn = memo(
  function Turn({
    page,
    turn,
    hasProjectRoot,
    onOpenResource,
    onRetry,
  }: {
    page: AcpChatPage;
    turn: TurnProjection;
    hasProjectRoot: boolean;
    onOpenResource?: AcpChatPresentationProps["onOpenResource"];
    onRetry?: () => void;
  }) {
    const primary = turn.items.filter(
      (item) => item.origin === "primary" && item.kind !== "fileChangeDiff",
    );
    const activityIds = new Set(turn.activities.map((activity) => activity.id));
    const itemActivities = new Map(
      turn.items.map((item) => [item.id, item.activityId] as const),
    );
    const roots = turn.activities.filter(
      (activity) =>
        !activity.parentActivityId ||
        !activityIds.has(activity.parentActivityId) ||
        itemActivities.get(activity.parentItemId) !== activity.parentActivityId,
    );
    const interactive = turn.sessionId === page.currentSessionId;
    const fileReferenceOwner: AgentArtifactOwner | undefined =
      hasProjectRoot && page.projectId && page.chatId && page.currentSessionId
        ? {
            projectId: page.projectId,
            chatId: page.chatId,
            sessionId: page.currentSessionId,
          }
        : undefined;
    const actionOrigin = useMemo(
      () => ({
        projectId: page.projectId,
        chatId: page.chatId,
        sessionId: turn.sessionId,
      }),
      [page.chatId, page.projectId, turn.sessionId],
    );
    const openForTurn = useCallback(
      (resource: ResourceRef, target: OpenResourceTarget = "file") =>
        onOpenResource?.(resource, target, actionOrigin),
      [actionOrigin, onOpenResource],
    );
    const terminal = terminalTurnStates.has(turn.state);
    const promptItems = primary.filter(
      (item) => item.kind === "text" && item.payload.role === "user",
    );
    const attachmentItems = promptAttachmentItems(primary);
    const attachmentIds = new Set(attachmentItems.map((item) => item.id));
    const chronologicalItems = primary
      .filter(
        (item) =>
          !attachmentIds.has(item.id) &&
          !(
            item.kind === "thoughtSummary" &&
            terminalItemLifecycles.has(item.lifecycle)
          ) &&
          !(item.kind === "text" && item.payload.role === "user") &&
          !(terminal && item.kind === "text" && !item.payload.text.trim()),
      )
      .sort(
        (left, right) =>
          (Number.isFinite(left.ordinal) ? left.ordinal : Infinity) -
          (Number.isFinite(right.ordinal) ? right.ordinal : Infinity),
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
        (item.kind !== "thoughtSummary" || item.id === liveThought?.id),
    );
    const contentItems = finalAnswer ? [finalAnswer] : [];
    return (
      <section
        className="acp-turn"
        data-turn-state={turn.state}
        aria-label={`Turn ${turn.ordinal}`}
        data-turn-id={turn.id}
        data-session-id={turn.sessionId}
        data-runtime-epoch={turn.runtimeEpoch}
        data-turn-ordinal={turn.ordinal}
      >
        <TurnHeader turn={turn} />
        <div className="acp-turn__items">
          {attachmentItems.length > 0 ? (
            <div
              className="acp-turn__attachments"
              aria-label="Prompt attachments"
            >
              {attachmentItems.map((item) => (
                <ItemView key={item.id} item={item} interactive={interactive} />
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
                interactive={interactive}
                onOpenResource={
                  interactive && onOpenResource ? openForTurn : undefined
                }
                renderItem={(item) => (
                  <ItemView
                    key={item.id}
                    item={item}
                    workingItems={turn.items}
                    interactive={interactive}
                    fileReferenceOwner={fileReferenceOwner}
                    onOpenResource={
                      interactive && onOpenResource ? openForTurn : undefined
                    }
                  />
                )}
              />
            ) : null}
            <BrowserOperations
              operations={turn.browserOperations ?? []}
              collapsed={turn.browserOperationsCollapsed ?? 0}
            />
          </ChatTranscriptTurnSummary>
          <PrimaryActivities
            items={contentItems}
            interactive={interactive}
            onOpenResource={
              interactive && onOpenResource ? openForTurn : undefined
            }
            renderItem={(item) => (
              <ItemView
                key={item.id}
                item={item}
                turnState={turn.state}
                workingItems={turn.items}
                interactive={interactive}
                fileReferenceOwner={fileReferenceOwner}
                onOpenResource={
                  interactive && onOpenResource ? openForTurn : undefined
                }
              />
            )}
          />
          {roots.map((activity) => (
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
                  onOpenResource={
                    interactive && onOpenResource ? openForTurn : undefined
                  }
                />
              )}
            />
          ))}
          {/* docs/work-items/CHAT-WAVE-2/plan.md:28: live edits precede file data. */}
          {!terminal &&
          interactive &&
          turn.items.some(
            (item) =>
              item.kind === "fileChangeDiff" &&
              !terminalItemLifecycles.has(item.lifecycle) &&
              itemOwnershipMatches(page, turn, item),
          ) &&
          !turn.items.some(
            (item) =>
              item.kind === "toolCall" &&
              item.toolState === "inProgress" &&
              item.payload.presentation.kind === "edit",
          ) ? (
            <div
              className="acp-editing-files acp-tool-rollup__summary-copy"
              role="status"
              aria-live="polite"
            >
              <FilePenLine size={14} aria-hidden="true" />
              <span className="acp-live-shimmer">Editing files</span>
            </div>
          ) : null}
          <TurnFailure turn={turn} onRetry={onRetry} />
        </div>
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
    left.hasProjectRoot === right.hasProjectRoot &&
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
    <TurnEditSummary
      key={page.projectId + ":" + page.chatId + ":" + page.currentSessionId}
      page={page}
      rootLabels={labels}
      countIncomplete={countIncomplete}
      onOpenDiff={onOpenResource ? onOpenDiff : undefined}
      onOpenFile={onOpenResource ? onOpenFile : undefined}
      onRecordDisposition={acpDesktop.recordAgentEditDisposition}
    />
  );
  const renderTurn = (turn: TurnProjection, index: number) => {
    const timestamp = dayTimestamps[index];
    return (
      <Fragment key={turn.id}>
        <TurnDaySeparator timestamp={timestamp} />
        <Turn
          page={page}
          turn={turn}
          hasProjectRoot={projectRoots.length > 0}
          onOpenResource={onOpenResource}
          onRetry={index === page.turns.length - 1 ? onRetry : undefined}
        />
      </Fragment>
    );
  };
  return (
    <>
      {hasEarlier && onLoadEarlier ? (
        <button
          type="button"
          className="acp-history-action"
          disabled={loadingEarlier}
          onClick={onLoadEarlier}
        >
          {loadingEarlier ? "Loading earlier history…" : "Load earlier history"}
        </button>
      ) : null}
      {historyError ? (
        <p className="acp-history-error" role="alert" aria-live="off">
          Earlier history could not be loaded: {historyError}
        </p>
      ) : null}
      {page.nextCursor !== null ? (
        <p className="acp-history-notice">
          Earlier history has not been loaded. Changed-file summaries show
          retained records only.
        </p>
      ) : null}
      {retentionIncomplete ? (
        <p className="acp-history-notice">
          Some Chat history was discarded to stay within memory limits.
          Changed-file summaries show retained records only.
        </p>
      ) : null}
      {page.turns.length > 40 ? (
        <VirtualTurnList
          key={`${page.projectId}:${page.chatId}`}
          turns={page.turns}
          renderTurn={renderTurn}
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

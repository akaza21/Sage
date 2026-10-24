import { Fragment, memo, useCallback, useMemo, useRef } from "react";
import { createPortal } from "react-dom";

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
import { projectRootLabelKey, projectRootLabels } from "./agentEditSummary";
import { TurnEditSummary } from "./TurnEditSummary";
import { TurnFileActivity } from "./TurnFileActivity";
import { isEditingFiles } from "./editingFiles";
import { HarnessActivity } from "./HarnessActivity";
import { promptAttachmentItems } from "./acpTurnState";
import type { OpenResourceTarget } from "./ToolItem";
import { PrimaryActivities } from "./PrimaryActivities";
import { ToolRunningIndicator } from "./ToolRunningIndicator";
import { ItemView } from "./TurnActivityItem";
import { TurnHeader } from "./TurnHeader";
import { TurnFailure } from "./TurnFailure";
import { BrowserOperations } from "./BrowserOperations";
import { terminalItemLifecycles, terminalTurnStates } from "./statusSemantics";
import { VirtualTurnList } from "./VirtualTurnList";
import type { TurnListHost } from "../chatScroll";
import { NavigationPart, TranscriptPart } from "../lazyParts";
import { isSideChatId } from "../../../workbench/sideChatState";
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
    inherited = false,
  }: {
    page: AcpChatPage;
    turn: TurnProjection;
    /** A fork's read-only parent Turn: never interactive or quotable. */
    inherited?: boolean;
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
    const interactive = !inherited && turn.sessionId === page.currentSessionId;
    const fileReferenceOwner: AgentArtifactOwner | undefined =
      hasProjectRoot && page.projectId && page.chatId && page.currentSessionId
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
    const hasCollapsedActivities = workingItems.some(
      (item) => item.kind === "toolCall",
    );
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
                <ItemView key={item.id} item={item} interactive={interactive} />
              ))}
            </div>
          ) : null}
          {promptItems.map((item) => (
            <ItemView key={item.id} item={item} interactive={interactive} />
          ))}
          <ChatTranscriptTurnSummary
            turn={turn}
            thinkingPlaceholder={isSideChatId(page.chatId)}
            hasCollapsedActivities={hasCollapsedActivities}
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
                expandDetails={terminal}
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
                  onOpenResource={openForTurn}
                />
              )}
            />
          ))}
          {/* docs/work-items/CHAT-WAVE-2/plan.md:28: live edits precede file data. */}
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
          {terminal ? <TurnFileActivity page={page} turn={turn} /> : null}
          {stopped ? <TurnHeader turn={turn} /> : null}
          {stopped && !finalAnswer && onRetry ? (
            <button type="button" onClick={onRetry}>
              Retry
            </button>
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
    // `inherited` is fixed per mount: only InheritedTurns passes it.
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
      <NavigationPart
        part="ChatNavigation"
        // Distinct from the VirtualTurnList sibling's key: a shared key makes
        // React drop one of them and remount the other (closing find).
        key={`navigation:${page.projectId}:${page.chatId}`}
        turns={page.turns}
        chatId={page.chatId}
        hostRef={navigationRef}
        incomplete={hasEarlier || countIncomplete}
      />
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

/**
 * A fork's inherited parent history: turns only. It mounts no navigator,
 * find controller, virtual list or edit summary, so the fork's own
 * transcript keeps the only ones on its message list.
 */
export function InheritedTurns({ page }: { page: AcpChatPage }) {
  const dayTimestamps = useTurnDayTimestamps(page.turns);
  return page.turns.map((turn, index) => (
    <Fragment key={turn.id}>
      <TurnDaySeparator timestamp={dayTimestamps[index]} />
      <Turn page={page} turn={turn} hasProjectRoot={false} inherited />
    </Fragment>
  ));
}

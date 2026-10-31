import { memo, useMemo } from "react";
import type { AgentArtifactOwner } from "../../../domain/agentArtifact";
import type { ResourceRef } from "../../../domain/workbench";
import { AnswerActions } from "../AnswerActions";
import { ChatTranscriptTurnSummary } from "../ChatTranscriptFooter";
import { DeferredTurnFileActivity as TurnFileActivity } from "./DeferredChatDetails";
import { isEditingFiles } from "./editingFiles";
import { HarnessActivity } from "./HarnessActivity";
import {
  chronologicalTurnItems,
  promptAttachmentItems,
  rootActivities,
} from "./acpTurnState";
import type { OpenResourceTarget } from "./ToolItem";
import { PrimaryActivities } from "./PrimaryActivities";
import { ToolRunningIndicator } from "./ToolRunningIndicator";
import { ItemView } from "./TurnActivityItem";
import { TurnHeader } from "./TurnHeader";
import { ContextHandoffIndicator } from "./ContextHandoffIndicator";
import { sameTurnHandoffs } from "./turnHandoffs";
import { TurnFailure } from "./TurnFailure";
import { BrowserOperations } from "./BrowserOperations";
import { terminalTurnStates } from "./statusSemantics";
import { TranscriptPart } from "../lazyParts";
import { isSideChatId } from "../../../workbench/sideChatState";
import type { AcpChatTurnProps } from "./acpChatPresentationTypes";

export const AcpTranscriptTurn = memo(
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
  }: AcpChatTurnProps) {
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
    // An inherited Turn is inert, all but its answer's own row (s2/f0066).
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
          {page.handoffs
            ?.filter((handoff) => handoff.turnId === turn.id)
            .map((handoff) => (
              <ContextHandoffIndicator key={handoff.id} summary={handoff} />
            ))}
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
              thinkingPlaceholder={isSideChatId(page.chatId)}
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
    sameTurnHandoffs(left.page, right.page, left.turn.id) &&
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

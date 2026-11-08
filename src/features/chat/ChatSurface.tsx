import { useContext, useEffect, useRef } from "react";
import { useRetainedElement } from "../../ui/useRetainedElement";
import {
  AgentSessionChromeContext,
  usePinnedAgentChatOwner,
} from "../../app/agentChatOwner";
import { useWorkbenchStore } from "../../app/store";
import type { TurnProjection } from "../../domain/acp";
import { ChatComposerRegion } from "./ChatComposerRegion";
import { ChatHandoffControl } from "./ChatHandoffControl";
import { ChatTranscript } from "./ChatTranscript";
import { SessionWorkingPill } from "./SessionWorkingPill";
import {
  latestRetryPrompt,
  sessionActionAvailable,
  sessionActions,
  terminalTurnStates,
  useAcpChatActions,
  useAcpChatHistory,
  useAcpChatLoadFailure,
  useAcpRetentionIncomplete,
  useRendererRecreationFocus,
} from "./acp";
import { useComposerFocusHotkey } from "./composerChrome";
import { LazyHarnessNotice } from "./lazyParts";
import { isSideChatId } from "../../workbench/sideChatState";
import { useAgentSessionChromePublisher } from "./useAgentSessionChromePublisher";
import { useApprovalSlot } from "./useApprovalSlot";
import { useBoundChatComposer } from "./useBoundChatComposer";
import { useChatHarnessSelection } from "./useChatHarnessSelection";
import { useChatSurfaceModel } from "./useChatSurfaceModel";
import { useFollowEnd } from "./useFollowEnd";
export function ChatSurface({ chatId }: { chatId?: string } = {}) {
  const focusedChatId = usePinnedAgentChatOwner();
  const setAgentSessionChrome = useContext(AgentSessionChromeContext);
  const messages = useWorkbenchStore((state) => state.messages);
  const activeTabId = useWorkbenchStore((state) => state.activeTabId);
  const harnesses = useWorkbenchStore((state) => state.harnesses);
  const {
    page: acpPage,
    selectedHarnessId,
    selectedModelId,
    harnessDefinition: selectedHarnessDefinition,
    discoveryPhase,
    chooseHarness,
    chooseModel,
  } = useChatHarnessSelection(chatId);
  const {
    composerText,
    attachedContext,
    imagePreviews,
    imageUploading,
    addImages,
    queuedPrompts,
    sending,
    chatError,
    setComposerText,
    removeAttachedContext,
    sendPrompt,
    queuePrompt,
    updateQueuedPrompt,
    steerQueuedPrompt,
  } = useBoundChatComposer(chatId, acpPage ?? null);
  const projectTransitioning = useWorkbenchStore((s) => s.projectTransitioning);
  const epoch = useWorkbenchStore((state) => state.nativeTransitionEpoch);
  const project = useWorkbenchStore((state) => state.project);
  const error = useWorkbenchStore((state) => state.error);
  const acpRetentionIncomplete = useAcpRetentionIncomplete(chatId);
  const acpLoadFailure = useAcpChatLoadFailure(chatId);
  const acpActions = useAcpChatActions(chatId);
  const acpHistory = useAcpChatHistory(chatId);
  const surfaceRef = useRef<HTMLElement>(null);
  const messageListRef = useRef<HTMLDivElement>(null);
  const transcriptContentRef = useRef<HTMLDivElement>(null);
  const endRef = useRef<HTMLDivElement>(null);
  const composerRef = useRef<HTMLTextAreaElement>(null);
  const [editSummaryContainer, setEditSummaryContainer] =
    useRetainedElement<HTMLDivElement>();
  const priorActiveTurnId = useRef<TurnProjection["id"] | null>(null);
  const terminalFocusTurnId = useRef<TurnProjection["id"] | null>(null);
  const composerFieldId = chatId ? `prompt-${chatId}` : "prompt";
  const focused = chatId == null || focusedChatId === chatId;
  const {
    selectedHarness,
    useNativeAcp,
    availableBinding,
    harnessStatus,
    foregroundError,
    modeError,
    modeChanging,
    actionBusy,
    activeAcpTurn,
    workingTurnCount,
    canCloseSession,
    canEndSession,
    canPrompt,
    sessionHarnessId,
    accessBindingId,
    sessionModelId,
    sessionPromptUnavailable,
    usage,
    composerEnabled,
    sendBlocked,
    queueAvailable,
    composerHotkeyEnabled,
    disabledReason,
    submit,
    closeSession,
    endSession,
    cancelTurn,
    steerAvailable,
    openResource,
  } = useChatSurfaceModel({
    harnesses,
    selectedHarnessId,
    selectedModelId,
    page: acpPage,
    discoveryPhase,
    composerText,
    attachedContext,
    imageUploading,
    sending,
    chatError,
    chatLoadFailed: Boolean(acpLoadFailure.error),
    projectTransitioning,
    focused,
    acpActions,
    sendPrompt,
    queuePrompt,
    queuedPromptCount: queuedPrompts.length,
    queueSavePending: queuedPrompts.some(
      (prompt) => prompt.status === "saving",
    ),
  });
  useAgentSessionChromePublisher(
    setAgentSessionChrome,
    chatId,
    focusedChatId,
    {
      harnessStatus,
      canCloseSession,
      canEndSession,
      busy: actionBusy || Boolean(activeAcpTurn) || projectTransitioning,
      acpPage: useNativeAcp ? (acpPage ?? null) : null,
      messages,
    },
    { closeSession, endSession },
    composerRef,
  );
  const approval = useApprovalSlot(
    activeAcpTurn,
    chatId ?? activeTabId,
    composerRef,
    { focused, onCancelTurn: cancelTurn ?? undefined },
  );
  useRendererRecreationFocus(surfaceRef, focused ? acpPage : null);
  useComposerFocusHotkey(composerRef, composerHotkeyEnabled);
  const { showJumpToLatest, updateFollowEnd, jumpToLatest } = useFollowEnd({
    ownerKey: chatId ?? "primary",
    revision: `${messages.length}:${acpPage?.projectionRevision ?? ""}`,
    running: Boolean(activeAcpTurn),
    transcriptRef: messageListRef,
    contentRef: transcriptContentRef,
    endRef,
  });
  const sessionAction = (kind: keyof typeof sessionActions) => {
    const action = sessionActions[kind];
    return useNativeAcp &&
      !actionBusy &&
      !projectTransitioning &&
      sessionActionAvailable(acpPage, action.kind)
      ? () => void acpActions.runSessionAction(action)
      : undefined;
  };
  const retryLastTurn = () => {
    if (!acpPage || actionBusy || activeAcpTurn) return;
    const retry = latestRetryPrompt(acpPage);
    if (retry)
      void acpActions.submit(retry.prompt, [], retry.bindingId, retry.modelId);
  };

  useEffect(() => {
    const priorId = priorActiveTurnId.current;
    const priorTurn = priorId
      ? acpPage?.turns.find((turn) => turn.id === priorId)
      : null;
    if (priorId && priorTurn && terminalTurnStates.has(priorTurn.state)) {
      terminalFocusTurnId.current = priorId;
      priorActiveTurnId.current = activeAcpTurn?.id ?? null;
    } else if (activeAcpTurn) {
      priorActiveTurnId.current = activeAcpTurn.id;
    } else if (priorId && !priorTurn) {
      priorActiveTurnId.current = null;
    }
    if (
      focused &&
      terminalFocusTurnId.current &&
      !actionBusy &&
      !projectTransitioning
    ) {
      const surface = surfaceRef.current;
      const activeElement = document.activeElement;
      const preserveFocus =
        activeElement !== surface && surface?.contains(activeElement);
      const composer = composerRef.current;
      const statuses =
        surface?.querySelectorAll<HTMLElement>("[data-turn-status]");
      const terminalStatus = [...(statuses ?? [])].find(
        (status) => status.dataset.turnStatus === terminalFocusTurnId.current,
      );
      const target = composer && !composer.disabled ? composer : terminalStatus;
      if (!preserveFocus) target?.focus();
      if (target || preserveFocus) terminalFocusTurnId.current = null;
    }
  }, [
    acpPage,
    activeAcpTurn,
    actionBusy,
    canPrompt,
    focused,
    projectTransitioning,
  ]);

  return (
    <section
      ref={surfaceRef}
      className={`chat-surface${setAgentSessionChrome ? " chat-surface--agent" : ""}`}
      aria-label="Agent chat"
    >
      {!setAgentSessionChrome ? (
        <header className="surface-header">
          <SessionWorkingPill
            count={workingTurnCount}
            hidden={projectTransitioning}
          />
          <div className="session-controls">
            {/* Side chats render only under Agent chrome, so this header never
                belongs to one. */}
            <ChatHandoffControl
              acpPage={useNativeAcp ? acpPage : null}
              messages={messages}
              busy={actionBusy || Boolean(activeAcpTurn)}
              onCommitted={() =>
                queueMicrotask(() => composerRef.current?.focus())
              }
            />
            {canCloseSession ? (
              <button
                className="button button--compact"
                type="button"
                disabled={actionBusy || projectTransitioning}
                onClick={closeSession}
              >
                Close session
              </button>
            ) : null}
            {canEndSession ? (
              <button
                className="button button--compact"
                type="button"
                disabled={actionBusy || projectTransitioning}
                onClick={endSession}
              >
                End session
              </button>
            ) : null}
            <div
              className="session-state"
              aria-label={`Harness status: ${harnessStatus}`}
            >
              {harnessStatus === "Live" ? (
                <span className="session-state__dot" />
              ) : null}
              {harnessStatus}
            </div>
          </div>
        </header>
      ) : null}
      <LazyHarnessNotice
        harness={selectedHarnessDefinition}
        discoveryPhase={discoveryPhase}
      />

      <ChatTranscript
        editSummaryContainer={editSummaryContainer}
        transcriptRef={messageListRef}
        contentRef={transcriptContentRef}
        endRef={endRef}
        onScroll={updateFollowEnd}
        projectTransitioning={projectTransitioning}
        project={project}
        useNativeAcp={useNativeAcp}
        page={acpPage}
        focused={focused}
        projectRoots={project?.roots ?? []}
        retentionIncomplete={acpRetentionIncomplete}
        onOpenResource={openResource}
        {...acpHistory}
        messages={messages}
        developmentSimulator={selectedHarness?.source === "development"}
        onRetry={
          useNativeAcp && !projectTransitioning && !actionBusy && !activeAcpTurn
            ? retryLastTurn
            : undefined
        }
        onReviewChanges={sessionAction("reviewChanges")}
        showJumpToLatest={showJumpToLatest}
        onJumpToLatest={jumpToLatest}
      />

      <ChatComposerRegion
        approvalSlot={approval.slot}
        onEditSummaryMount={setEditSummaryContainer}
        hidden={approval.awaiting}
        composerRef={composerRef}
        fieldId={composerFieldId}
        dictationScope={chatId ?? activeTabId ?? ""}
        sessionId={acpPage?.currentSessionId ?? null}
        descriptionId={`${composerFieldId}-description`}
        disabledReasonId={`${composerFieldId}-disabled-reason`}
        text={composerText}
        attachedContext={attachedContext}
        imagePreviews={imagePreviews}
        imageUploading={imageUploading}
        onAddImages={addImages}
        enabled={composerEnabled}
        sendBlocked={sendBlocked}
        actionBusy={actionBusy}
        activeTurn={Boolean(activeAcpTurn)}
        queueAvailable={queueAvailable}
        queuedPrompts={queuedPrompts}
        steerAvailable={steerAvailable}
        projectTransitioning={projectTransitioning}
        workbenchError={error}
        chatError={foregroundError}
        claudeSignInRequired={
          selectedHarnessId === "claude-code" &&
          acpActions.errorKind === "signInRequired"
        }
        claudeAuthScope={`${project?.id ?? ""}:${epoch}:${chatId ?? activeTabId ?? ""}`}
        accessBindingId={accessBindingId}
        availableBinding={availableBinding}
        canPrompt={canPrompt}
        discoveryPhase={discoveryPhase}
        sessionHarnessId={sessionHarnessId}
        sessionModelId={sessionModelId}
        selectedHarnessId={selectedHarnessId}
        selectedModelId={selectedModelId}
        sessionPromptUnavailable={sessionPromptUnavailable}
        sessionConfig={acpPage?.sessionConfig ?? null}
        modePending={modeChanging}
        modeError={modeError}
        focused={focused}
        onSetSessionMode={acpActions.setSessionMode}
        onSetSessionConfigOption={acpActions.setSessionConfigOption}
        onCompact={sessionAction("compact")}
        onSelectHarness={chooseHarness}
        onSelectModel={chooseModel}
        missingBindingNotice={useNativeAcp && !availableBinding}
        disabledReason={disabledReason}
        onRetryLoad={acpLoadFailure.error ? acpLoadFailure.retry : null}
        // The project chip keeps one place in the toolbar before and after
        // the first turn. A side chat belongs to its parent's project and
        // cannot switch it, so it has no chip.
        project={isSideChatId(acpPage?.chatId ?? chatId) ? null : project}
        usage={usage}
        goal={(useNativeAcp && acpPage?.goal) || { kind: "unsupported" }}
        onSubmit={submit}
        onTextChange={setComposerText}
        onRemoveContext={removeAttachedContext}
        onCancelTurn={cancelTurn}
        onDeleteQueuedPrompt={(id) => updateQueuedPrompt(id, "deleted")}
        onRetryQueuedPrompt={(id) => updateQueuedPrompt(id, "queued")}
        onSteerQueuedPrompt={(id) => steerQueuedPrompt(id, acpActions.steer)}
      />
    </section>
  );
}

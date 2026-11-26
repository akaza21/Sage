import { useContext, useRef } from "react";
import { useRetainedElement } from "../../ui/useRetainedElement";
import {
  AgentSessionChromeContext,
  usePinnedAgentChatOwner,
} from "../../app/agentChatOwner";
import { useWorkbenchStore } from "../../app/store";
import { composerProblem } from "./ComposerStatusNotices";
import { ChatComposerRegion } from "./ChatComposerRegion";
import { ChatTranscript } from "./ChatTranscript";
import { useUsageRequest } from "./acp/useUsageRequest";
import { recoverStart } from "./acp/sessionRequest";
import {
  latestRetryPrompt,
  sessionActionAvailable,
  sessionActions,
  useAcpChatActions,
  useAcpChatHistory,
  useAcpChatLoadFailure,
  useAcpRetentionIncomplete,
  useRendererRecreationFocus,
} from "./acp";
import { useComposerFocusHotkey } from "./composerChrome";
import { lazyParts } from "./lazyParts";
import { isSideChatId } from "../../workbench/sideChatState";
import { isNewProjectChat } from "./newProjectChat";
import { useAgentSessionChromePublisher } from "./useAgentSessionChromePublisher";
import { useApprovalSlot } from "./useApprovalSlot";
import { useBoundChatComposer } from "./useBoundChatComposer";
import { useChatHarnessSelection } from "./useChatHarnessSelection";
import { useChatSurfaceModel } from "./useChatSurfaceModel";
import { useFollowEnd } from "./useFollowEnd";
import { useTerminalTurnFocus } from "./useTerminalTurnFocus";
import {
  restoredSessionConfigView,
  withSessionConfigNotice,
} from "./restoredSessionConfigView";
import type * as SessionHeaderChunk from "./ChatSessionHeader";
const SessionHeaderPart = lazyParts<typeof SessionHeaderChunk>(
  () => import("./ChatSessionHeader"),
);
import { SlashPart } from "./composerLazyParts";

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
  const composerFieldId = chatId ? `prompt-${chatId}` : "prompt";
  const focused = chatId == null || focusedChatId === chatId;
  const {
    selectedHarness,
    selectedSession,
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
    accessTarget,
    sessionModelId,
    sessionPromptUnavailable,
    usage,
    composerEnabled,
    sendBlocked,
    queueAvailable,
    composerHotkeyEnabled,
    disabledReason,
    imageInput,
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
  const usageRequest = useUsageRequest(useNativeAcp ? acpPage : null, epoch);
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
  const retry = acpPage && latestRetryPrompt(acpPage);
  // Offered only while idle and with a retry (see onRetry), and it closes over
  // that same render's values.
  const retryLastTurn = () => {
    if (retry!.action) return void acpActions.runSessionAction(retry!.action);
    // Retry resends what that turn carried, never the next one's paste.
    const { prompt, bindingId, modelId, turnId } = retry!;
    void acpActions.submit(prompt, [], bindingId, modelId, { retryOf: turnId });
  };

  useTerminalTurnFocus({
    page: acpPage,
    activeTurn: activeAcpTurn,
    actionBusy,
    canPrompt,
    focused,
    projectTransitioning,
    surfaceRef,
    composerRef,
  });
  const firstSendPending = useWorkbenchStore((state) =>
    Boolean(chatId && state.pendingFirstSend?.chatId === chatId),
  );
  const newChat = isNewProjectChat({
    useNativeAcp,
    page: acpPage,
    project,
    projectTransitioning,
    ...acpHistory,
    retentionIncomplete: acpRetentionIncomplete,
  });
  const boundSession =
    acpPage?.sessions.find(
      (session) => session.id === acpPage.currentSessionId,
    ) ?? acpPage?.sessions.at(-1);
  return (
    <section
      ref={surfaceRef}
      className={`chat-surface${setAgentSessionChrome ? " chat-surface--agent" : ""}`}
      aria-label="Agent chat"
    >
      {/* A pending first send is always this chat's (see firstSendPending). */}
      {firstSendPending ? (
        <SlashPart
          part="FirstChatSend"
          chatId={chatId!}
          page={acpPage}
          actions={acpActions}
        />
      ) : null}
      {!setAgentSessionChrome ? (
        <SessionHeaderPart
          part="ChatSessionHeader"
          page={useNativeAcp ? acpPage : null}
          messages={messages}
          workingTurnCount={workingTurnCount}
          projectTransitioning={projectTransitioning}
          busy={actionBusy || Boolean(activeAcpTurn)}
          canCloseSession={canCloseSession}
          canEndSession={canEndSession}
          closeSession={closeSession}
          endSession={endSession}
          harnessStatus={harnessStatus}
          onCommitted={() => queueMicrotask(() => composerRef.current?.focus())}
        />
      ) : null}

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
          useNativeAcp &&
          !projectTransitioning &&
          !actionBusy &&
          !activeAcpTurn &&
          retry &&
          (!retry.action || sessionActionAvailable(acpPage, retry.action.kind))
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
        commandChatId={chatId}
        sessionId={acpPage?.currentSessionId ?? null}
        descriptionId={`${composerFieldId}-description`}
        disabledReasonId={`${composerFieldId}-disabled-reason`}
        text={composerText}
        attachedContext={attachedContext}
        pasteSession={selectedSession}
        imagePreviews={imagePreviews}
        imageUploading={imageUploading}
        imageInputAvailable={imageInput.available}
        imageInputDisabledReason={imageInput.reason}
        onAddImages={useNativeAcp ? addImages : undefined}
        enabled={composerEnabled && !firstSendPending}
        sendBlocked={sendBlocked}
        actionBusy={actionBusy || firstSendPending}
        activeTurn={Boolean(activeAcpTurn) || firstSendPending}
        onStop={
          firstSendPending
            ? () => useWorkbenchStore.setState({ pendingFirstSend: null })
            : undefined
        }
        queueAvailable={queueAvailable}
        queuedPrompts={queuedPrompts}
        steerAvailable={steerAvailable}
        projectTransitioning={projectTransitioning}
        workbenchError={composerProblem(
          "workspace",
          [project?.id, epoch],
          error,
        )}
        chatError={composerProblem(
          acpActions.errorKind ?? "command",
          [chatId ?? activeTabId, acpPage?.currentSessionId],
          foregroundError,
        )}
        chatAuth={{
          page: acpPage,
          errorKind: acpActions.errorKind,
          boundChatId: chatId,
        }}
        accessTarget={accessTarget}
        availableBinding={availableBinding}
        canPrompt={canPrompt}
        discoveryPhase={discoveryPhase}
        sessionHarnessId={sessionHarnessId}
        sessionModelId={sessionModelId}
        selectedHarnessId={selectedHarnessId}
        selectedModelId={selectedModelId}
        sessionPromptUnavailable={sessionPromptUnavailable}
        sessionConfig={withSessionConfigNotice(
          acpPage?.sessionConfig ??
            restoredSessionConfigView(
              acpPage,
              selectedHarness?.bindingId,
              selectedModelId,
            ),
          acpPage?.sessionConfigNotice,
        )}
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
        retryNotice={
          acpLoadFailure.error
            ? { onRetry: acpLoadFailure.retry }
            : sessionPromptUnavailable && selectedSession?.state === "starting"
              ? {
                  onRetry: () => recoverStart(chatId),
                  message: "The agent did not finish starting.",
                }
              : null
        }
        project={isSideChatId(acpPage?.chatId ?? chatId) ? null : project}
        newChat={newChat && !isSideChatId(acpPage?.chatId ?? chatId)}
        usage={usage}
        usageRequest={usageRequest}
        goal={(useNativeAcp && acpPage?.goal) || { kind: "unsupported" }}
        goalCapabilities={
          boundSession?.runtimeEpoch
            ? boundSession.capabilities.goal
            : undefined
        }
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

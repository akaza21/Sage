import type { FormEventHandler, RefObject } from "react";

import type { HarnessDiscoveryPhase } from "../../app/harnessSlice";
import type { AcpGoalState } from "../../domain/acpGoal";
import type {
  AcpConfigValue,
  AcpSessionConfig,
} from "../../domain/acpSessionConfig";
import type { AttachedContext } from "../../domain/attachedContext";
import type { QueuedChatPrompt } from "../../domain/chatOwnership";
import { MAX_CHAT_COMPOSER_BYTES } from "../../domain/chatOwnership";
import { keymapBinding, matchesKeymapChord } from "../../domain/keymapRegistry";
import { isTranscriptQuote } from "../../domain/transcriptQuote";
import { Folder } from "../../ui/leanIcons";
import "../../styles/queued-messages.css";
import type { ChatUsage } from "./acp/chatUsage";
import { BoundChatGoalControl } from "./BoundChatGoalControl";
import { ChatHarnessControls } from "./ChatHarnessControls";
import type * as ClaudeAuthChunk from "./ClaudeAuthActions";
import { ComposerAttachments } from "./ComposerAttachments";
import {
  composerPromptPlaceholder,
  composerShortcutLabel,
  composerWorkspaceName,
  useComposerDictation,
} from "./composerChrome";
import { ComposerContext } from "./ComposerContext";
import { ComposerImageInput } from "./ComposerImageInput";
import { ComposerTurnControl } from "./ComposerTurnControl";
import { QueuedRows, lazyParts } from "./lazyParts";
import { TranscriptPart } from "./transcriptLazy";
import { useBoundGoalDetails } from "./useBoundGoalDetails";
import { useComposerImageInput } from "./useComposerImageInput";

const AuthPart = lazyParts<typeof ClaudeAuthChunk>(
  () => import("./ClaudeAuthActions"),
);

export function ChatComposerRegion({
  hidden = false,
  composerRef,
  fieldId,
  dictationScope,
  sessionId = null,
  descriptionId,
  disabledReasonId,
  text,
  attachedContext,
  imagePreviews,
  imageUploading = false,
  enabled,
  sendBlocked = false,
  actionBusy,
  activeTurn,
  queueAvailable,
  queuedPrompts,
  steerAvailable,
  projectTransitioning,
  workbenchError,
  chatError,
  claudeSignInRequired,
  claudeAuthScope,
  accessBindingId,
  availableBinding,
  canPrompt,
  discoveryPhase,
  sessionHarnessId,
  sessionModelId,
  selectedHarnessId,
  selectedModelId,
  sessionPromptUnavailable,
  sessionConfig,
  modePending,
  modeError,
  focused,
  onSetSessionMode,
  onSetSessionConfigOption,
  onSelectHarness,
  onSelectModel,
  missingBindingNotice,
  disabledReason,
  onRetryLoad,
  project,
  projectScopeName = null,
  usage,
  goal,
  onSubmit,
  onTextChange,
  onRemoveContext,
  imageInputAvailable = false,
  onAddImages,
  onCancelTurn,
  onDeleteQueuedPrompt,
  onRetryQueuedPrompt,
  onSteerQueuedPrompt,
}: {
  /** Kept mounted while an approval holds the slot, so the draft survives. */
  hidden?: boolean;
  composerRef: RefObject<HTMLTextAreaElement | null>;
  fieldId: string;
  dictationScope: string;
  sessionId?: string | null;
  descriptionId: string;
  disabledReasonId: string;
  text: string;
  attachedContext: readonly AttachedContext[];
  imagePreviews?: Readonly<Record<string, string>>;
  imageUploading?: boolean;
  enabled: boolean;
  sendBlocked?: boolean;
  actionBusy: boolean;
  activeTurn: boolean;
  queueAvailable?: boolean;
  queuedPrompts?: readonly QueuedChatPrompt[];
  steerAvailable?: boolean;
  projectTransitioning: boolean;
  workbenchError: string | null;
  chatError: string | null;
  claudeSignInRequired?: boolean;
  claudeAuthScope?: string;
  accessBindingId?: string | null;
  availableBinding: boolean;
  canPrompt: boolean;
  discoveryPhase: HarnessDiscoveryPhase;
  sessionHarnessId: string | null;
  sessionModelId: string | null;
  selectedHarnessId: string | null;
  selectedModelId: string | null;
  sessionPromptUnavailable: boolean;
  sessionConfig: AcpSessionConfig | null;
  modePending: boolean;
  modeError: string | null;
  focused: boolean;
  onSetSessionMode: (modeId: string) => Promise<unknown>;
  onSetSessionConfigOption: (
    configId: string,
    value: AcpConfigValue,
  ) => Promise<unknown>;
  onSelectHarness: (harnessId: string) => Promise<void>;
  onSelectModel: (modelId: string) => void;
  missingBindingNotice: boolean;
  disabledReason: string | null;
  onRetryLoad?: (() => void) | null;
  project: { name: string } | null | undefined;
  projectScopeName?: string | null;
  usage: ChatUsage | null;
  goal: AcpGoalState;
  onSubmit: FormEventHandler<HTMLFormElement>;
  onTextChange: (text: string) => void;
  onRemoveContext: (context: AttachedContext) => void;
  imageInputAvailable?: boolean;
  onAddImages?: (files: File[]) => void;
  onCancelTurn: (() => void) | null;
  onDeleteQueuedPrompt?: (id: string) => void;
  onRetryQueuedPrompt?: (id: string) => void;
  onSteerQueuedPrompt?: (id: string) => void;
}) {
  const shortcutLabel = composerShortcutLabel(/mac/i.test(navigator.platform));
  const composerDisabled = !enabled || projectTransitioning;
  const goalDetails = useBoundGoalDetails(
    goal,
    dictationScope,
    availableBinding && sessionHarnessId === selectedHarnessId,
  );
  // Transcript quotes render in their own lazy chip row above the textarea.
  const otherContext = attachedContext.filter((c) => !isTranscriptQuote(c));
  const imageInput = useComposerImageInput({
    contexts: attachedContext,
    available: imageInputAvailable,
    disabled: composerDisabled,
    uploading: imageUploading,
    onAddImages,
  });
  const dispatchBlocked =
    composerDisabled ||
    sendBlocked ||
    actionBusy ||
    (activeTurn && !queueAvailable) ||
    imageUploading ||
    imageInput.sendBlocked;
  const queueing = activeTurn && queueAvailable && !composerDisabled;
  const shortcutDescriptionId = `${descriptionId}-shortcut`;
  const dictation = useComposerDictation(
    text,
    onTextChange,
    dictationScope,
    composerDisabled || activeTurn,
    composerRef,
  );
  return (
    <form
      className="composer"
      role="form"
      hidden={hidden}
      aria-busy={projectTransitioning || imageUploading}
      onDragOver={imageInput.onDragOver}
      onDrop={imageInput.onDrop}
      onPaste={imageInput.onPaste}
      onSubmit={(event) => {
        if (dispatchBlocked) {
          event.preventDefault();
          return;
        }
        dictation.cancel();
        onSubmit(event);
        composerRef.current?.focus();
      }}
    >
      {projectScopeName ? (
        <div
          className="composer__scope"
          role="group"
          aria-label="Project scope"
        >
          <Folder size={14} aria-hidden="true" />
          <span>{projectScopeName}</span>
        </div>
      ) : null}
      {otherContext.length < attachedContext.length ? (
        <TranscriptPart
          part="ComposerAnnotations"
          contexts={attachedContext}
          disabled={actionBusy || projectTransitioning}
          onRemove={onRemoveContext}
          chatId={dictationScope}
        />
      ) : null}
      <div className="composer__body">
        {goal.kind === "goal" ? (
          <BoundChatGoalControl
            state={goal}
            chatId={dictationScope}
            detailsRef={goalDetails.detailsRef}
            canMutate={goalDetails.canMutate}
          />
        ) : null}
        <QueuedRows
          composerRef={composerRef}
          prompts={queuedPrompts}
          steerAvailable={steerAvailable}
          disabled={actionBusy || projectTransitioning}
          onDelete={onDeleteQueuedPrompt}
          onRetry={onRetryQueuedPrompt}
          onSteer={onSteerQueuedPrompt}
        />
        {workbenchError ? (
          <div className="composer__error" role="alert">
            {workbenchError}
          </div>
        ) : null}
        {chatError ? (
          <div className="composer__error" role="alert">
            {chatError}
            {claudeSignInRequired ? (
              <AuthPart part="ClaudeAuthActions" key={claudeAuthScope} />
            ) : null}
          </div>
        ) : null}
        {imageInput.status ? (
          <div className="composer__error" role="alert">
            {imageInput.status}
          </div>
        ) : null}
        {missingBindingNotice &&
        !["checking", "signInRequired", "consent", "waiting", "error"].includes(
          discoveryPhase,
        ) ? (
          <div className="composer__notice">
            No reviewed local binding is available for this harness.
          </div>
        ) : null}
        <label className="sr-only" htmlFor={fieldId}>
          Message agent
        </label>
        <span className="sr-only" id={descriptionId}>
          {queueing ? "Queue" : "Send"} a message to the selected harness and
          model
        </span>
        {!composerDisabled ? (
          <span className="sr-only" id={shortcutDescriptionId}>
            {shortcutLabel} focuses the composer.{" "}
            {queueing
              ? "Enter queues a message while this turn runs."
              : "Enter sends."}{" "}
            Shift+Enter adds a new line.
          </span>
        ) : null}
        <div className="composer__input-area">
          <textarea
            ref={composerRef}
            id={fieldId}
            aria-label="Message agent"
            aria-description={
              queueing
                ? "Enter queues a message while this turn runs."
                : undefined
            }
            aria-describedby={`${descriptionId}${composerDisabled ? "" : ` ${shortcutDescriptionId}`}${disabledReason ? ` ${disabledReasonId}` : ""}`}
            value={text}
            maxLength={MAX_CHAT_COMPOSER_BYTES}
            rows={2}
            placeholder={composerPromptPlaceholder(
              canPrompt,
              sessionPromptUnavailable,
            )}
            disabled={composerDisabled}
            onChange={(event) => onTextChange(event.target.value)}
            onBlur={dictation.cancel}
            onKeyDown={(event) => {
              if (event.key === "Escape" && dictation.listening) {
                event.preventDefault();
                dictation.cancel();
                return;
              }
              if (
                enabled &&
                matchesKeymapChord(keymapBinding("sendMessage"), event)
              ) {
                event.preventDefault();
                event.currentTarget.form?.requestSubmit();
              }
            }}
          />
        </div>
        {disabledReason ? (
          <p className="composer__disabled-reason" id={disabledReasonId}>
            {disabledReason}
            {onRetryLoad ? (
              <button
                type="button"
                className="composer__disabled-retry"
                onClick={onRetryLoad}
              >
                Retry
              </button>
            ) : null}
          </p>
        ) : null}
        {otherContext.length > 0 ? (
          <div className="composer__attachment-row">
            <ComposerAttachments
              contexts={otherContext}
              imagePreviews={imagePreviews}
              disabled={actionBusy || projectTransitioning}
              onRemove={onRemoveContext}
            />
          </div>
        ) : null}
      </div>
      <div className="composer__toolbar">
        {imageInputAvailable ? (
          <ComposerImageInput
            inputRef={imageInput.chooserRef}
            disabled={composerDisabled || imageUploading}
            onChoose={imageInput.accept}
          />
        ) : null}
        <ChatHarnessControls
          modelLabelScope={JSON.stringify([dictationScope, sessionId])}
          inheritScope={dictationScope}
          accessBindingId={accessBindingId}
          onOpenGoal={goalDetails.onOpen}
          imageAction={imageInput.action}
          availableBinding={availableBinding}
          canPrompt={canPrompt}
          discoveryPhase={discoveryPhase}
          sessionHarnessId={sessionHarnessId}
          sessionModelId={sessionModelId}
          selectedHarnessId={selectedHarnessId}
          selectedModelId={selectedModelId}
          projectName={composerWorkspaceName(project)}
          turnBusy={actionBusy || activeTurn}
          sessionConfig={sessionConfig}
          modePending={modePending}
          modeError={modeError}
          focused={focused}
          onSetSessionMode={onSetSessionMode}
          onSetSessionConfigOption={onSetSessionConfigOption}
          onSelectHarness={onSelectHarness}
          onSelectModel={onSelectModel}
          onModeRemoved={() => composerRef.current?.focus()}
        />
        {!composerDisabled ? (
          <span className="composer__shortcut" aria-hidden="true">
            {shortcutLabel} to focus <span>/</span>{" "}
            {queueing ? "Enter to queue" : "Enter to send"}
          </span>
        ) : null}
        <ComposerContext usage={usage} />
        <ComposerTurnControl
          dictation={dictation}
          disabled={composerDisabled}
          onCancelTurn={onCancelTurn}
          cancelDisabled={actionBusy || projectTransitioning}
          queueing={Boolean(activeTurn && queueAvailable)}
          sendDisabled={
            dispatchBlocked || (!text.trim() && !attachedContext.length)
          }
        />
      </div>
    </form>
  );
}

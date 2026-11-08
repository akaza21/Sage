import { useState } from "react";
import type { ChatComposerRegionProps } from "./chatComposerRegionProps";
import { MAX_CHAT_COMPOSER_BYTES } from "../../domain/chatOwnership";
import { keymapBinding, matchesKeymapChord } from "../../domain/keymapRegistry";
import { utf8ByteLength } from "../../domain/text";
import { isTranscriptQuote } from "../../domain/transcriptQuote";
import "../../styles/queued-messages.css";
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
import { ComposerImageInput } from "./ComposerImageInput";
import { ComposerTurnControl } from "./ComposerTurnControl";
import { QueuedRows, lazyParts } from "./lazyParts";
import { TranscriptPart } from "./transcriptLazy";
import { useBoundGoalDetails } from "./useBoundGoalDetails";
import { useComposerImageInput } from "./useComposerImageInput";
import type * as PasteChunk from "./ComposerPaste";
import type { StagedSummary } from "./pastedTextTypes";

const AuthPart = lazyParts<typeof ClaudeAuthChunk>(
  () => import("./ClaudeAuthActions"),
);
// Paste staging, chips and messages load with the field's first focus or
// with input over the cap; only the synchronous size check stays eager.
const PastePart = lazyParts<typeof PasteChunk>(() => import("./ComposerPaste"));
/** A string longer than the cap in UTF-16 units is over it in bytes too. */
const overCap = (value: string) =>
  value.length > MAX_CHAT_COMPOSER_BYTES ||
  utf8ByteLength(value) > MAX_CHAT_COMPOSER_BYTES;

export function ChatComposerRegion({
  hidden = false,
  approvalSlot,
  onEditSummaryMount,
  composerRef,
  fieldId,
  dictationScope,
  sessionId = null,
  descriptionId,
  disabledReasonId,
  text,
  attachedContext,
  pasteSession,
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
}: ChatComposerRegionProps) {
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
  // The paste part loads with the first focus (true) or held-back input,
  // and reports the batch it staged for the next sent message.
  const [paste, setPaste] = useState<string | boolean>(false);
  const [staged, setStaged] = useState<StagedSummary>();
  const dispatchBlocked =
    composerDisabled ||
    sendBlocked ||
    actionBusy ||
    (activeTurn && !queueAvailable) ||
    imageUploading ||
    imageInput.sendBlocked ||
    // Staged pasted text goes with a sent message, never a queued one.
    Boolean(staged?.busy || (activeTurn && staged?.revision));
  const queueing = activeTurn && queueAvailable && !composerDisabled;
  const shortcutDescriptionId = `${descriptionId}-shortcut`;
  const dictation = useComposerDictation(
    text,
    onTextChange,
    dictationScope,
    hidden || composerDisabled || activeTurn,
    composerRef,
  );
  return (
    <div className="chat-dock surface surface--dock">
      <div className="chat-edit-summary-zone" ref={onEditSummaryMount} />
      {approvalSlot}
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
          onSubmit(event, staged?.revision);
          composerRef.current?.focus();
        }}
      >
        <div className="composer__slots">
          {otherContext.length < attachedContext.length ? (
            <TranscriptPart
              part="ComposerAnnotations"
              contexts={attachedContext}
              disabled={actionBusy || projectTransitioning}
              onRemove={onRemoveContext}
              chatId={dictationScope}
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
        </div>
        <div className="composer__body">
          {goal.kind === "goal" ? (
            <BoundChatGoalControl
              state={goal}
              chatId={dictationScope}
              detailsRef={goalDetails.detailsRef}
              canMutate={goalDetails.canMutate}
            />
          ) : null}
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
          ![
            "checking",
            "signInRequired",
            "consent",
            "waiting",
            "error",
          ].includes(discoveryPhase) ? (
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
              spellCheck={false}
              autoCorrect="off"
              autoCapitalize="off"
              aria-description={
                queueing
                  ? "Enter queues a message while this turn runs."
                  : undefined
              }
              aria-describedby={`${descriptionId}${composerDisabled ? "" : ` ${shortcutDescriptionId}`}${disabledReason ? ` ${disabledReasonId}` : ""}`}
              value={text}
              rows={2}
              placeholder={composerPromptPlaceholder(
                canPrompt,
                sessionPromptUnavailable,
              )}
              disabled={composerDisabled}
              onFocus={() => setPaste((armed) => armed || true)}
              onChange={(event) => {
                const next = event.target.value;
                // Held back with a message, never truncated. A draft is
                // never over the cap, so this never blocks a deletion. Once
                // loaded, the paste part attaches a longer paste instead.
                if (overCap(next)) setPaste(next);
                else onTextChange(next);
              }}
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
          {paste ? (
            <PastePart
              part="ComposerPaste"
              chatId={dictationScope}
              fieldRef={composerRef}
              session={pasteSession}
              nativeAcp={availableBinding}
              activeTurn={activeTurn}
              input={paste}
              text={text}
              disabled={actionBusy || projectTransitioning}
              onStaged={setStaged}
            />
          ) : null}
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
          {!hidden && (
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
              contextUsage={usage}
            />
          )}
          {!composerDisabled ? (
            <span className="composer__shortcut" aria-hidden="true">
              {shortcutLabel} to focus <span>/</span>{" "}
              {queueing ? "Enter to queue" : "Enter to send"}
            </span>
          ) : null}
          <ComposerTurnControl
            dictation={dictation}
            disabled={composerDisabled}
            onCancelTurn={onCancelTurn}
            cancelDisabled={actionBusy || projectTransitioning}
            queueing={Boolean(activeTurn && queueAvailable)}
            sendDisabled={
              dispatchBlocked ||
              (!text.trim() && !attachedContext.length && !staged?.revision)
            }
          />
        </div>
      </form>
    </div>
  );
}

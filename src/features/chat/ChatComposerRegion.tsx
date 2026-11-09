import { useEffect, useRef, useState, type ComponentProps } from "react";
import type { ChatComposerRegionProps } from "./chatComposerRegionProps";
import { isImeComposing } from "../../ui/imeComposition";
import { keymapBinding, matchesKeymapChord } from "../../domain/keymapRegistry";
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
import { useComposerPasteSignal } from "./useComposerPasteSignal";
import { ComposerPasteNotices } from "./ComposerPasteNotices";
import type * as SlashChunk from "./ComposerSlashController";

const AuthPart = lazyParts<typeof ClaudeAuthChunk>(
  () => import("./ClaudeAuthActions"),
);
// Paste staging, chips and messages load with a native chat's composer or
// with input over the cap; only the synchronous size check stays eager.
const PastePart = lazyParts<typeof PasteChunk>(
  () => import("./ComposerPaste"),
  { ComposerPaste: PasteUnavailable },
);

/**
 * Stands in for a paste part that failed to load: nothing can be staged,
 * so held input is let go, sends are not held for a batch, and a standing
 * notice says why long text is refused. The next composer mount tries the
 * import again.
 */
function PasteUnavailable({
  input: { seq, take },
  noticeIds,
  onStaged,
}: ComponentProps<typeof PasteChunk.ComposerPaste>) {
  useEffect(() => void take(), [seq, take]);
  useEffect(
    () => onStaged({ revision: null, busy: false, settled: true }),
    [onStaged],
  );
  return (
    <ComposerPasteNotices
      ids={noticeIds}
      reason="Text over the message limit cannot be attached: attachment support failed to load."
    />
  );
}

// The `/` command menu loads with the field's first focus.
const SlashPart = lazyParts<typeof SlashChunk>(
  () => import("./ComposerSlashController"),
);

export function ChatComposerRegion({
  hidden = false,
  approvalSlot,
  onEditSummaryMount,
  composerRef,
  fieldId,
  dictationScope,
  commandChatId,
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
  signInHarness = null,
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
  onCompact,
  onSelectHarness,
  onSelectModel,
  missingBindingNotice,
  disabledReason,
  onRetryLoad,
  project,
  usage,
  usageRequest,
  goal,
  onSubmit,
  onTextChange,
  onRemoveContext,
  imageInputAvailable = false,
  imageInputDisabledReason = null,
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
    unavailableReason: imageInputDisabledReason,
    disabled: composerDisabled,
    uploading: imageUploading,
    onAddImages,
  });
  // The paste part loads with the composer of a native chat, or with
  // held-back input, and reports the batch it staged.
  const paste = useComposerPasteSignal();
  const [staged, setStaged] = useState<StagedSummary>();
  // A native chat loads it at once, so the batch staged before a reload is
  // read back before any send, whether or not the field was focused.
  const pasteArmed = availableBinding || paste.seq > 0;
  const pasteNoticeIds = {
    notice: `${disabledReasonId}-paste`,
    reason: `${disabledReasonId}-paste-reason`,
  };
  const dispatchBlocked =
    composerDisabled ||
    sendBlocked ||
    actionBusy ||
    (activeTurn && !queueAvailable) ||
    imageUploading ||
    imageInput.sendBlocked ||
    // Staged pasted text goes with a sent message, never a queued one, and
    // no send goes before the batch staged earlier has been read back.
    Boolean(
      staged?.busy ||
      (activeTurn && staged?.revision) ||
      (pasteArmed && !staged?.settled),
    );
  const queueing = activeTurn && queueAvailable && !composerDisabled;
  const shortcutDescriptionId = `${descriptionId}-shortcut`;
  const dictation = useComposerDictation(
    text,
    onTextChange,
    dictationScope,
    hidden || composerDisabled || activeTurn,
    composerRef,
  );
  const slashReady = useRef(false);
  const [slashArmed, setSlashArmed] = useState(false);
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
          {chatError || signInHarness ? (
            <div className="composer__error" role="alert">
              {signInHarness ? (
                <AuthPart
                  part="SignInRequiredMessage"
                  harness={signInHarness}
                  fallback={chatError}
                />
              ) : (
                chatError
              )}
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
              // A combobox for the `/` command menu (ARIA 1.2); its menu
              // controller owns aria-expanded once loaded.
              role="combobox"
              aria-expanded={false}
              spellCheck={false}
              autoCorrect="off"
              autoCapitalize="off"
              aria-description={
                queueing
                  ? "Enter queues a message while this turn runs."
                  : undefined
              }
              aria-describedby={`${descriptionId}${composerDisabled ? "" : ` ${shortcutDescriptionId}`}${disabledReason ? ` ${disabledReasonId}` : ""}${pasteArmed ? ` ${pasteNoticeIds.notice} ${pasteNoticeIds.reason}` : ""}`}
              value={text}
              rows={2}
              placeholder={composerPromptPlaceholder(
                canPrompt,
                sessionPromptUnavailable,
              )}
              disabled={composerDisabled}
              onPaste={paste.onPaste}
              onChange={(event) => {
                const next = event.target.value;
                // Held back with a message, never truncated. A draft is
                // never over the cap, so this never blocks a deletion.
                if (!paste.holdEdit(next)) onTextChange(next);
              }}
              onFocus={() => setSlashArmed(true)}
              onBlur={dictation.cancel}
              onKeyDown={(event) => {
                // An IME composition owns its keys, Enter included.
                if (isImeComposing(event)) return;
                // Until the `/` menu loads, Enter holds a half-typed name.
                if (
                  !slashReady.current &&
                  event.key === "Enter" &&
                  !event.shiftKey &&
                  /^\/\S*$/u.test(text)
                ) {
                  event.preventDefault();
                  return;
                }
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
            {slashArmed ? (
              <SlashPart
                part="ComposerSlashController"
                fieldRef={composerRef}
                readyRef={slashReady}
                boundChatId={commandChatId}
                disabled={hidden || composerDisabled}
                fieldId={fieldId}
                text={text}
                onTextChange={onTextChange}
              />
            ) : null}
          </div>
          {pasteArmed ? (
            <PastePart
              part="ComposerPaste"
              chatId={dictationScope}
              session={pasteSession}
              nativeAcp={availableBinding}
              activeTurn={activeTurn}
              input={paste}
              text={text}
              disabled={actionBusy || projectTransitioning}
              noticeIds={pasteNoticeIds}
              onStaged={setStaged}
              fallback={<ComposerPasteNotices ids={pasteNoticeIds} />}
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
              addActions={{
                imageAction: imageInput.action,
                onOpenGoal: goalDetails.onOpen,
                onCompact,
              }}
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
              usageRequest={usageRequest}
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

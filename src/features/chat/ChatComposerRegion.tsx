import { ArrowUp, Mic, MoreHorizontal, Square, X } from "../../ui/leanIcons";

import type { FormEventHandler, RefObject } from "react";

import type { ChatUsage } from "./acp/chatUsage";
import type { HarnessDiscoveryPhase } from "../../app/harnessSlice";
import type {
  AcpConfigValue,
  AcpSessionConfig,
} from "../../domain/acpSessionConfig";
import type { AttachedContext } from "../../domain/attachedContext";
import { MAX_CHAT_COMPOSER_BYTES } from "../../domain/chatOwnership";
import type { QueuedChatPrompt } from "../../domain/chatOwnership";
import { keymapBinding, matchesKeymapChord } from "../../domain/keymapRegistry";
import { ChatHarnessControls } from "./ChatHarnessControls";
import { BoundChatGoalControl } from "./BoundChatGoalControl";
import type { AcpGoalState } from "../../domain/acpGoal";
import { QueuedAttachmentPreview } from "./ComposerAttachments";
import {
  composerPromptPlaceholder,
  composerShortcutLabel,
  composerWorkspaceName,
  useComposerDictation,
} from "./composerChrome";
import { ComposerAttachments } from "./ComposerAttachments";
import { ComposerContext } from "./ComposerContext";
import { ClaudeAuthActions } from "./ClaudeAuthActions";
import "../../styles/queued-messages.css";

export function ChatComposerRegion({
  composerRef,
  fieldId,
  dictationScope,
  descriptionId,
  disabledReasonId,
  text,
  attachedContext,
  enabled,
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
  availableBinding,
  canPrompt,
  discoveryPhase,
  sessionHarnessId,
  sessionModelId,
  selectedHarnessId,
  selectedModelId,
  sessionPromptUnavailable,
  sessionConfig,
  permissionRequests,
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
  onCancelTurn,
  onDeleteQueuedPrompt,
  onRetryQueuedPrompt,
  onSteerQueuedPrompt,
}: {
  composerRef: RefObject<HTMLTextAreaElement | null>;
  fieldId: string;
  dictationScope: string;
  descriptionId: string;
  disabledReasonId: string;
  text: string;
  attachedContext: readonly AttachedContext[];
  enabled: boolean;
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
  availableBinding: boolean;
  canPrompt: boolean;
  discoveryPhase: HarnessDiscoveryPhase;
  sessionHarnessId: string | null;
  sessionModelId: string | null;
  selectedHarnessId: string | null;
  selectedModelId: string | null;
  sessionPromptUnavailable: boolean;
  sessionConfig: AcpSessionConfig | null;
  permissionRequests: boolean;
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
  usage: ChatUsage | null;
  goal: AcpGoalState;
  onSubmit: FormEventHandler<HTMLFormElement>;
  onTextChange: (text: string) => void;
  onRemoveContext: (context: AttachedContext) => void;
  onCancelTurn: (() => void) | null;
  onDeleteQueuedPrompt?: (id: string) => void;
  onRetryQueuedPrompt?: (id: string) => void;
  onSteerQueuedPrompt?: (id: string) => void;
}) {
  const shortcutLabel = composerShortcutLabel(/mac/i.test(navigator.platform));
  const composerDisabled = !enabled || actionBusy || projectTransitioning;
  const queueing = activeTurn && queueAvailable && !composerDisabled;
  const shortcutDescriptionId = `${descriptionId}-shortcut`;
  const projectName = composerWorkspaceName(project);
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
      aria-busy={projectTransitioning}
      onSubmit={(event) => {
        dictation.cancel();
        onSubmit(event);
      }}
    >
      <BoundChatGoalControl state={goal} chatId={dictationScope} />
      {queuedPrompts && queuedPrompts.length > 0 ? (
        <div className="composer__queued" aria-label="Queued messages">
          {queuedPrompts.map((prompt) => (
            <div className="composer__queued-row" key={prompt.id}>
              <QueuedAttachmentPreview contexts={prompt.attachedContext} />
              <span className="composer__queued-text" title={prompt.text}>
                {prompt.text}
              </span>
              <span className="composer__queued-state">
                {prompt.status === "saving"
                  ? "Saving…"
                  : prompt.status === "deleting"
                    ? "Removing…"
                    : prompt.status === "blocked"
                      ? "Needs retry"
                      : prompt.status === "outcomeUnknown"
                        ? "May have been delivered"
                        : prompt.status === "sending"
                          ? "Sending…"
                          : "Queued"}
              </span>
              {steerAvailable &&
              prompt.status !== "sending" &&
              prompt.status !== "saving" &&
              prompt.status !== "deleting" ? (
                <button
                  type="button"
                  className="composer__queued-steer"
                  aria-label={`Steer queued message: ${prompt.text}`}
                  title={
                    prompt.status === "outcomeUnknown"
                      ? "Retry Steer with the same message ID"
                      : "Submit without interrupting the model"
                  }
                  disabled={actionBusy || projectTransitioning}
                  onClick={() => onSteerQueuedPrompt?.(prompt.id)}
                >
                  Steer
                </button>
              ) : null}
              <button
                type="button"
                aria-label={`Delete queued message: ${prompt.text}`}
                disabled={
                  prompt.status === "saving" || prompt.status === "deleting"
                }
                onClick={() => onDeleteQueuedPrompt?.(prompt.id)}
              >
                <X size={16} aria-hidden="true" />
              </button>
              <details className="composer__queued-options">
                <summary
                  aria-label={`More options for queued message: ${prompt.text}`}
                >
                  <MoreHorizontal size={16} aria-hidden="true" />
                </summary>
                <button
                  type="button"
                  onClick={() =>
                    prompt.status === "blocked"
                      ? onRetryQueuedPrompt?.(prompt.id)
                      : onDeleteQueuedPrompt?.(prompt.id)
                  }
                >
                  {prompt.status === "blocked" ? "Retry when ready" : "Delete"}
                </button>
              </details>
            </div>
          ))}
        </div>
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
            <ClaudeAuthActions key={claudeAuthScope} />
          ) : null}
        </div>
      ) : null}
      {missingBindingNotice ? (
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
          aria-describedby={`${descriptionId}${composerDisabled ? "" : ` ${shortcutDescriptionId}`}${disabledReason ? ` ${disabledReasonId}` : ""}`}
          value={text}
          maxLength={MAX_CHAT_COMPOSER_BYTES}
          rows={2}
          placeholder={
            queueing
              ? "Enter to queue a message"
              : composerPromptPlaceholder(canPrompt, sessionPromptUnavailable)
          }
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
      {attachedContext.length > 0 ? (
        <div className="composer__attachment-row">
          <ComposerAttachments
            contexts={attachedContext}
            disabled={actionBusy || projectTransitioning}
            onRemove={onRemoveContext}
          />
        </div>
      ) : null}
      <div className="composer__toolbar">
        <ChatHarnessControls
          availableBinding={availableBinding}
          canPrompt={canPrompt}
          discoveryPhase={discoveryPhase}
          sessionHarnessId={sessionHarnessId}
          sessionModelId={sessionModelId}
          selectedHarnessId={selectedHarnessId}
          selectedModelId={selectedModelId}
          projectName={projectName}
          turnBusy={actionBusy || activeTurn}
          sessionConfig={sessionConfig}
          permissionRequests={permissionRequests}
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
        <button
          className="composer__mic"
          type="button"
          aria-label={
            !dictation.supported
              ? "Voice input unavailable"
              : dictation.listening
                ? "Stop dictation"
                : "Start dictation"
          }
          title={
            dictation.supported
              ? "Speech may be processed by an online service"
              : "Voice input is unavailable in this WebView"
          }
          disabled={
            !dictation.supported || (composerDisabled && !dictation.listening)
          }
          aria-pressed={dictation.listening}
          onMouseDown={(event) => event.preventDefault()}
          onClick={dictation.listening ? dictation.stop : dictation.start}
        >
          <Mic size={16} aria-hidden="true" />
        </button>
        {dictation.error ? (
          <span className="composer__dictation-error" role="alert">
            {dictation.error}
          </span>
        ) : null}
        {onCancelTurn ? (
          <button
            className="send-button send-button--cancel"
            type="button"
            aria-label="Cancel turn"
            disabled={actionBusy || projectTransitioning}
            onClick={onCancelTurn}
          >
            <Square size={14} fill="currentColor" aria-hidden="true" />
          </button>
        ) : null}
        {!onCancelTurn ? (
          <button
            className="send-button"
            type="submit"
            aria-label={
              activeTurn && queueAvailable ? "Queue message" : "Send message"
            }
            disabled={
              projectTransitioning || !text.trim() || !enabled || actionBusy
            }
          >
            <ArrowUp size={20} aria-hidden="true" />
          </button>
        ) : null}
      </div>
    </form>
  );
}

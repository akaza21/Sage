import { ArrowUp } from "lucide-react";
import type { FormEventHandler, RefObject } from "react";

import type { AcpItem } from "../../domain/acp";
import type { AcpSessionConfig } from "../../domain/acpSessionConfig";
import type { DiffLineCommentContext } from "../../domain/attachedContext";
import { MAX_CHAT_COMPOSER_BYTES } from "../../domain/chatOwnership";
import { ChatHarnessControls } from "./ChatHarnessControls";
import {
  composerPromptPlaceholder,
  composerShortcutLabel,
} from "./composerChrome";
import { ComposerAffordances } from "./ComposerAffordances";
import { ComposerAttachments } from "./ComposerAttachments";
import { ComposerContext } from "./ComposerContext";
import { composerWorkspaceName } from "./composerChrome";
import { HarnessDiscoveryNotice } from "./HarnessDiscoveryNotice";

export function ChatComposerRegion({
  composerRef,
  fieldId,
  descriptionId,
  disabledReasonId,
  text,
  attachedContext,
  enabled,
  actionBusy,
  projectTransitioning,
  workbenchError,
  chatError,
  availableBinding,
  canPrompt,
  sessionConfig,
  modePending,
  modeError,
  focused,
  onSetSessionMode,
  missingBindingNotice,
  disabledReason,
  project,
  usage,
  onSubmit,
  onTextChange,
  onRemoveContext,
  onCancelTurn,
}: {
  composerRef: RefObject<HTMLTextAreaElement | null>;
  fieldId: string;
  descriptionId: string;
  disabledReasonId: string;
  text: string;
  attachedContext: readonly DiffLineCommentContext[];
  enabled: boolean;
  actionBusy: boolean;
  projectTransitioning: boolean;
  workbenchError: string | null;
  chatError: string | null;
  availableBinding: boolean;
  canPrompt: boolean;
  sessionConfig: AcpSessionConfig | null;
  modePending: boolean;
  modeError: string | null;
  focused: boolean;
  onSetSessionMode: (modeId: string) => Promise<unknown>;
  missingBindingNotice: boolean;
  disabledReason: string | null;
  project: { name: string } | null | undefined;
  usage: Extract<AcpItem, { kind: "usage" }>["payload"] | null;
  onSubmit: FormEventHandler<HTMLFormElement>;
  onTextChange: (text: string) => void;
  onRemoveContext: (context: DiffLineCommentContext) => void;
  onCancelTurn: (() => void) | null;
}) {
  const shortcutLabel = composerShortcutLabel(/mac/i.test(navigator.platform));
  const composerDisabled = !enabled || actionBusy || projectTransitioning;
  const projectName = composerWorkspaceName(project);
  const shortcutDescriptionId = `${descriptionId}-shortcut`;

  return (
    <form
      className="composer"
      aria-busy={projectTransitioning}
      onSubmit={onSubmit}
    >
      {workbenchError ? (
        <div className="composer__error" role="alert">
          {workbenchError}
        </div>
      ) : null}
      {chatError ? (
        <div className="composer__error" role="alert">
          {chatError}
        </div>
      ) : null}
      <HarnessDiscoveryNotice />
      {missingBindingNotice ? (
        <div className="composer__notice">
          No reviewed local binding is available for this harness.
        </div>
      ) : null}
      <label className="sr-only" htmlFor={fieldId}>
        Message agent
      </label>
      <span className="sr-only" id={descriptionId}>
        Send a message to the selected harness and model
      </span>
      {!composerDisabled ? (
        <span className="sr-only" id={shortcutDescriptionId}>
          {shortcutLabel} focuses the composer. Enter sends. Shift+Enter adds a
          new line.
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
          placeholder={composerPromptPlaceholder(canPrompt)}
          disabled={composerDisabled}
          onChange={(event) => onTextChange(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter" && !event.shiftKey && enabled) {
              event.preventDefault();
              event.currentTarget.form?.requestSubmit();
            }
          }}
        />
        <ComposerContext usage={usage} />
      </div>
      {disabledReason ? (
        <p className="composer__disabled-reason" id={disabledReasonId}>
          {disabledReason}
        </p>
      ) : null}
      <div className="composer__attachment-row">
        <ComposerAffordances />
        <ComposerAttachments
          contexts={attachedContext}
          disabled={actionBusy || projectTransitioning}
          onRemove={onRemoveContext}
        />
      </div>
      <div className="composer__toolbar">
        <ChatHarnessControls
          availableBinding={availableBinding}
          canPrompt={canPrompt}
          projectName={projectName}
          sessionConfig={sessionConfig}
          modePending={modePending}
          modeError={modeError}
          focused={focused}
          onSetSessionMode={onSetSessionMode}
          onModeRemoved={() => composerRef.current?.focus()}
        />
        {!composerDisabled ? (
          <span className="composer__shortcut" aria-hidden="true">
            {shortcutLabel} to focus <span>/</span> Enter to send
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
            Cancel
          </button>
        ) : (
          <button
            className="send-button"
            type="submit"
            aria-label="Send message"
            disabled={
              projectTransitioning || !text.trim() || !enabled || actionBusy
            }
          >
            <ArrowUp size={20} aria-hidden="true" />
          </button>
        )}
      </div>
    </form>
  );
}

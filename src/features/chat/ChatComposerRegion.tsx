import { ArrowUp } from "lucide-react";
import type { FormEventHandler, RefObject } from "react";

import type { AcpItem } from "../../domain/acp";
import { MAX_CHAT_COMPOSER_BYTES } from "../../domain/chatOwnership";
import { ChatHarnessControls } from "./ChatHarnessControls";
import {
  composerPromptPlaceholder,
  composerShortcutLabel,
} from "./composerChrome";
import { ComposerContext } from "./ComposerContext";
import { HarnessDiscoveryNotice } from "./HarnessDiscoveryNotice";

export function ChatComposerRegion({
  composerRef,
  fieldId,
  descriptionId,
  disabledReasonId,
  text,
  enabled,
  actionBusy,
  projectTransitioning,
  workbenchError,
  chatError,
  availableBinding,
  canPrompt,
  missingBindingNotice,
  disabledReason,
  project,
  usage,
  onSubmit,
  onTextChange,
  onCancelTurn,
}: {
  composerRef: RefObject<HTMLTextAreaElement | null>;
  fieldId: string;
  descriptionId: string;
  disabledReasonId: string;
  text: string;
  enabled: boolean;
  actionBusy: boolean;
  projectTransitioning: boolean;
  workbenchError: string | null;
  chatError: string | null;
  availableBinding: boolean;
  canPrompt: boolean;
  missingBindingNotice: boolean;
  disabledReason: string | null;
  project: { name: string } | null | undefined;
  usage: Extract<AcpItem, { kind: "usage" }>["payload"] | null;
  onSubmit: FormEventHandler<HTMLFormElement>;
  onTextChange: (text: string) => void;
  onCancelTurn: (() => void) | null;
}) {
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
      <textarea
        ref={composerRef}
        id={fieldId}
        aria-label="Message agent"
        aria-describedby={`${descriptionId}${disabledReason ? ` ${disabledReasonId}` : ""}`}
        value={text}
        maxLength={MAX_CHAT_COMPOSER_BYTES}
        rows={2}
        placeholder={composerPromptPlaceholder(
          canPrompt,
          composerShortcutLabel(/mac/i.test(navigator.platform)),
        )}
        disabled={!enabled || actionBusy || projectTransitioning}
        onChange={(event) => onTextChange(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Enter" && !event.shiftKey && enabled) {
            event.preventDefault();
            event.currentTarget.form?.requestSubmit();
          }
        }}
      />
      {disabledReason ? (
        <p className="composer__disabled-reason" id={disabledReasonId}>
          {disabledReason}
        </p>
      ) : null}
      <div className="composer__toolbar">
        <ChatHarnessControls
          availableBinding={availableBinding}
          canPrompt={canPrompt}
        />
        <span className="composer__shortcut" aria-hidden="true">
          Enter to send <span>/</span> Shift+Enter for a new line
        </span>
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
            <ArrowUp size={18} aria-hidden="true" />
          </button>
        )}
      </div>
      <ComposerContext project={project} usage={usage} />
    </form>
  );
}

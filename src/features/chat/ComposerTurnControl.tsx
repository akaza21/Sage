import type { ButtonHTMLAttributes } from "react";
import type { useComposerDictation } from "./composerChrome";
import { ArrowUp, Square, Mic } from "../../ui/leanIcons";

export function ComposerTurnControl({
  dictation,
  activeTurn,
  hasContent,
  onCancelTurn,
  cancelDisabled,
  sendDisabled,
  disabledReason,
  disabledReasonId,
}: {
  dictation: ReturnType<typeof useComposerDictation>;
  activeTurn: boolean;
  hasContent: boolean;
  onCancelTurn: (() => void) | null;
  cancelDisabled: boolean;
  sendDisabled: boolean;
  disabledReason?: string | null;
  disabledReasonId?: string;
}) {
  const reason = sendDisabled ? (disabledReason ?? undefined) : undefined;
  const microphone = !activeTurn && !hasContent;
  const Icon = microphone ? Mic : activeTurn ? Square : ArrowUp;
  const action: ButtonHTMLAttributes<HTMLButtonElement> = activeTurn
    ? {
        className: "send-button send-button--cancel",
        "aria-label": "Cancel turn",
        title: "Voice input unavailable during a turn",
        disabled: cancelDisabled || !onCancelTurn,
        onClick: onCancelTurn ?? undefined,
      }
    : microphone
      ? {
          className: "composer__mic",
          "aria-label": !dictation.supported
            ? "Voice input unavailable"
            : dictation.listening
              ? "Stop dictation"
              : "Start dictation",
          title: dictation.supported
            ? "May use an online service"
            : "Voice input is unavailable in this WebView",
          disabled: dictation.disabled,
          "aria-pressed": dictation.listening,
          onMouseDown: (event) => event.preventDefault(),
          onClick: dictation.listening ? dictation.stop : dictation.start,
        }
      : {
          className: "send-button",
          type: "submit",
          "aria-label": "Send message",
          title: reason,
          "aria-describedby": reason ? disabledReasonId : undefined,
          disabled: sendDisabled,
        };
  return (
    <>
      <button type="button" {...action}>
        <Icon aria-hidden="true" />
      </button>
      {dictation.error ? (
        <span className="composer__dictation-error" role="alert">
          {dictation.error}
        </span>
      ) : null}
    </>
  );
}

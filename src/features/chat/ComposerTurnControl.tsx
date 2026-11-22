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
  const reason = (sendDisabled && disabledReason) || undefined;
  const microphone = dictation.listening || !hasContent;
  const Icon = activeTurn ? Square : microphone ? Mic : ArrowUp;
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
          "aria-label": dictation.listening
            ? "Stop dictation"
            : dictation.supported
              ? "Start dictation"
              : "Voice input unavailable",
          title: dictation.supported
            ? "May use an online service"
            : "Voice input is unavailable in this WebView",
          disabled: dictation.disabled,
          "aria-pressed": dictation.listening,
          onMouseDown: (event) => event.preventDefault(),
          onClick: (event) => {
            // A trusted click re-renders this button as Send before its
            // activation runs, so cancel activation or Stop would submit.
            event.preventDefault();
            if (dictation.listening) dictation.stop();
            else void dictation.start();
          },
        }
      : {
          className: "send-button",
          type: "submit",
          "aria-label": "Send message",
          title: reason,
          "aria-describedby": reason && disabledReasonId,
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

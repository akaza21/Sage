import { useRef, type ButtonHTMLAttributes } from "react";
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
  const hadFocus = useRef(false);
  const reason = (sendDisabled && disabledReason) || undefined;
  const [name, Icon, action]: [
    "cancel" | "dictate" | "send",
    typeof Square,
    ButtonHTMLAttributes<HTMLButtonElement>,
  ] = activeTurn
    ? [
        "cancel",
        Square,
        {
          className: "send-button send-button--cancel",
          "aria-label": "Cancel turn",
          disabled: cancelDisabled || !onCancelTurn,
          onClick: onCancelTurn ?? undefined,
        },
      ]
    : dictation.listening || !hasContent
      ? [
          "dictate",
          Mic,
          {
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
            onClick: () =>
              dictation.listening ? dictation.stop() : void dictation.start(),
          },
        ]
      : [
          "send",
          ArrowUp,
          {
            className: "send-button",
            type: "submit",
            "aria-label": "Send message",
            title: reason,
            "aria-describedby": reason && disabledReasonId,
            disabled: sendDisabled,
          },
        ];
  return (
    <>
      {/* Each action remounts under its own key: a click needs press and
          release on one element, so a press on Stop cannot send. Keyboard
          focus follows the control across that remount, except out of
          Cancel: when a turn ends, useTerminalTurnFocus returns focus to the
          prompt. */}
      <button
        key={name}
        type="button"
        ref={(button) => {
          if (hadFocus.current) button?.focus();
          return () => {
            hadFocus.current =
              name !== "cancel" && document.activeElement === button;
          };
        }}
        {...action}
      >
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

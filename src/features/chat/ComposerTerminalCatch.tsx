import { useEffect, useRef, useState, type RefObject } from "react";
import { Notice } from "../../ui/Notice";
import { useWorkbenchStore } from "../../app/store";
import { isTauriRuntime } from "../../ports/nativeInvoke";
import { openTerminalWith } from "../../workbench/terminalDockHandoff";
import { useTerminalDockStore } from "../../app/terminalDockStore";
import {
  registerTerminalCatch,
  type CaughtTerminalCommand as Caught,
} from "../../services/terminalHandoff";

type Catcher = (caught: Caught) => boolean;

/**
 * Owns the one pending choice for a typed terminal-only command. Nothing is
 * sent until the user picks; editing the text releases the choice.
 */
export function ComposerTerminalCatch({
  chatId,
  fieldRef,
  onAnnounce,
  text,
}: {
  chatId: string;
  fieldRef: RefObject<HTMLTextAreaElement | null>;
  /** Posts to the field's status region, mounted before any catch. */
  onAnnounce: (message: string) => void;
  text: string;
}) {
  const [caught, setCaught] = useState<Caught | null>(null);
  const [opening, setOpening] = useState(false);
  const [error, setError] = useState("");
  const hasProject = useWorkbenchStore(
    (state) => !!state.project && !state.projectTransitioning,
  );
  const dockAvailable = useTerminalDockStore(
    (state) => state.ready && !!state.open,
  );
  const unavailable = !hasProject
    ? "Open this chat's project first."
    : !isTauriRuntime()
      ? "Terminals require the desktop app."
      : !dockAvailable
        ? "Terminal dock is unavailable or still loading."
        : undefined;
  const message =
    caught &&
    `/${caught.name} only runs in the terminal. Copy the command to resume this chat there, then type /${caught.name}.`;
  useEffect(() => onAnnounce(message || ""), [message, onAnnounce]);
  useEffect(() => () => onAnnounce(""), [onAnnounce]);
  // "Send as text anyway" consents to the next send of exactly this input.
  const consent = useRef("");
  if (caught && text.trim() !== caught.input) setCaught(null);
  // Any edit disarms a consent a queued or refused send left unused.
  useEffect(() => {
    if (consent.current !== text.trim()) consent.current = "";
  }, [text]);
  useEffect(() => {
    const take: Catcher = (next) => {
      const consented = consent.current === next.input;
      consent.current = "";
      // Only the prompt this field still shows; a queued send keeps its route.
      if (consented || fieldRef.current?.value.trim() !== next.input)
        return false;
      setCaught(next);
      setError("");
      return true;
    };
    return registerTerminalCatch(chatId, take);
  }, [chatId, fieldRef]);
  if (!caught) return null;
  const { input, name, command } = caught;
  return (
    // The shared Notice explains; its resolutions sit beside it, as for a
    // file conflict. The field's status region announces it.
    <div
      className="composer__terminal-command"
      role="group"
      aria-label="Terminal-only command"
    >
      <Notice
        identity={`terminal-command:${name}`}
        severity="info"
        message={message}
      />
      <span>
        <button
          type="button"
          className="button button--compact"
          disabled={opening || !!unavailable}
          title={unavailable}
          onClick={async () => {
            setOpening(true);
            setError("");
            try {
              await openTerminalWith({ chatId, text: command });
              setCaught((current) => (current === caught ? null : current));
            } catch (error) {
              setError(
                error instanceof Error
                  ? error.message
                  : "Terminal failed to open.",
              );
            } finally {
              setOpening(false);
            }
          }}
        >
          Open in terminal
        </button>
        <button
          type="button"
          className="button button--compact"
          onClick={() =>
            void navigator.clipboard.writeText(command).catch(() => {})
          }
        >
          Copy terminal command
        </button>
        <button
          type="button"
          className="button button--compact"
          onClick={() => {
            const field = fieldRef.current;
            consent.current = input;
            setCaught(null);
            // This choice unmounts itself, so focus returns to the field.
            field?.focus();
            field?.form?.requestSubmit();
          }}
        >
          Send as text anyway
        </button>
      </span>
      {error ? <span role="alert">{error}</span> : null}
    </div>
  );
}

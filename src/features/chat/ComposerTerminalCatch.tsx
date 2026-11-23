import { useEffect, useRef, useState, type RefObject } from "react";
import { Notice } from "../../ui/Notice";

type Caught = { input: string; name: string; command: string };
type Catcher = (caught: Caught) => boolean;
// One mounted composer field per chat owns its caught command.
const catchers = new Map<string, Catcher>();

/** The send path's hand-off: true when the chat's field took the command. */
// eslint-disable-next-line react-refresh/only-export-components -- the send path and the choice it opens share this chunk
export const catchTerminalCommand = (chatId: string, caught: Caught) =>
  !!catchers.get(chatId)?.(caught);

/**
 * Owns the one pending choice for a typed terminal-only command. Nothing is
 * sent until the user picks; editing the text releases the choice.
 */
export function ComposerTerminalCatch({
  chatId,
  fieldRef,
  text,
}: {
  chatId: string;
  fieldRef: RefObject<HTMLTextAreaElement | null>;
  text: string;
}) {
  const [caught, setCaught] = useState<Caught | null>(null);
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
      return true;
    };
    catchers.set(chatId, take);
    return () => {
      if (catchers.get(chatId) === take) catchers.delete(chatId);
    };
  }, [chatId, fieldRef]);
  if (!caught) return null;
  const { input, name, command } = caught;
  return (
    // The shared Notice explains; its resolutions sit beside it, as for a
    // file conflict. The region announces the choice when it appears.
    <div
      className="composer__terminal-command"
      role="status"
      aria-label="Terminal-only command"
    >
      <Notice
        identity={`terminal-command:${name}`}
        severity="info"
        message={`/${name} only runs in the terminal. Copy the command to resume this chat there, then type /${name}.`}
      />
      <span>
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
    </div>
  );
}

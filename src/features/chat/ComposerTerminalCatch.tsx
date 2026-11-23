import { useEffect, useRef, useState, type RefObject } from "react";
import { Notice } from "../../ui/Notice";

type Caught = { input: string; name: string; command: string };
// One mounted composer field per chat owns its caught command.
const catchers = new Map<string, (caught: Caught) => boolean>();

/** The send path's hand-off: true when the chat's field took the command. */
// eslint-disable-next-line react-refresh/only-export-components -- the send path and the choice it opens share this chunk
export const catchTerminalCommand = (chatId: string, caught: Caught) =>
  !!catchers.get(chatId)?.(caught);

/**
 * Owns the one pending choice for a typed terminal-only command. Nothing is
 * sent until the user picks; editing the text releases the choice. The
 * composer has no typed seam for the curated controls (the access menu,
 * model picker or add-on reload), so they keep this choice.
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
  // "Send as text anyway" consents to one send of exactly this input.
  const consent = useRef("");
  if (caught && text.trim() !== caught.input) setCaught(null);
  useEffect(() => {
    catchers.set(chatId, (next) => {
      const consented = consent.current === next.input;
      consent.current = "";
      // Only the prompt this field still shows; a queued send keeps its route.
      if (consented || fieldRef.current?.value.trim() !== next.input)
        return false;
      setCaught(next);
      return true;
    });
    return () => void catchers.delete(chatId);
  }, [chatId, fieldRef]);
  if (!caught) return null;
  const { input, name, command } = caught;
  return (
    // The shared Notice explains; its resolutions sit beside it, as for a
    // file conflict.
    <div
      className="composer__terminal-command"
      role="group"
      aria-label="Terminal-only command"
    >
      <Notice
        identity={`terminal-command:${name}`}
        severity="info"
        message={`/${name} only runs in the terminal.`}
      />
      <span>
        <button
          type="button"
          className="button button--compact"
          onClick={() =>
            void navigator.clipboard.writeText(`${command} /${name}`)
          }
        >
          Copy terminal command
        </button>
        <button
          type="button"
          className="button button--compact"
          onClick={() => {
            consent.current = input;
            setCaught(null);
            fieldRef.current?.form?.requestSubmit();
          }}
        >
          Send as text anyway
        </button>
      </span>
    </div>
  );
}

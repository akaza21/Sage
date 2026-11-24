import { useContext, useEffect, useRef, useState, type RefObject } from "react";
import { Notice } from "../../ui/Notice";
import { useWorkbenchStore, type TerminalHandoff } from "../../app/store";
import { AgentToolsContext } from "../../workbench/AgentToolsProvider";

type Caught = { input: string; name: string; command: string };
type Catcher = (caught: Caught) => boolean;
// A refused, timed-out or failed prefill: the user can still Copy.
const failed = "The terminal did not take the command.";
// The dock opens at most 4 tabs per chat.
const tabsFull = "4 terminals are already open.";
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
  const tools = useContext(AgentToolsContext);
  const unavailable = !useWorkbenchStore(
    (state) => !!state.project && !state.projectTransitioning,
  )
    ? "Open a project first."
    : !tools.terminalAvailable
      ? "No terminal here."
      : undefined;
  const message =
    caught &&
    `/${caught.name} only runs in the terminal. Copy the command to resume this chat there, then type /${caught.name}.`;
  useEffect(() => onAnnounce(message || ""), [message, onAnnounce]);
  useEffect(() => () => onAnnounce(""), [onAnnounce]);
  // An open request ends with its field: the slot and timer go with it.
  const cancel = useRef<() => void>(undefined);
  useEffect(() => () => cancel.current?.(), []);
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
    catchers.set(chatId, take);
    return () => {
      if (catchers.get(chatId) === take) catchers.delete(chatId);
    };
  }, [chatId, fieldRef]);
  if (!caught) return null;
  const { input, name, command } = caught;
  // Hands the Copy text to the lazy dock as typed, unrun input: one line, no
  // control character, so nothing presses Enter. A request that has not
  // settled in time fails here, whether or not the dock took it.
  const openInTerminal = () => {
    if (/\p{Cc}/u.test(command)) {
      setError(failed);
      return;
    }
    const store = useWorkbenchStore;
    const { project, nativeTransitionEpoch } = store.getState();
    const leave = () => {
      if (store.getState().terminalHandoff === handoff)
        store.setState({ terminalHandoff: null });
    };
    let settled = false;
    const settle = (failure?: string) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      leave();
      setOpening(false);
      if (failure) setError(failure);
      else setCaught((current) => (current === caught ? null : current));
    };
    // Only this chat's dock takes it. A request that already timed out or
    // ended types nothing, however late its tab opens.
    const handoff: TerminalHandoff = {
      projectId: project!.id,
      projectGeneration: String(nativeTransitionEpoch),
      chatId,
      run: (type) => {
        if (settled) return;
        leave();
        if (!type) return settle(tabsFull);
        void type(command).then(
          () => settle(),
          () => settle(failed),
        );
      },
    };
    const timer = setTimeout(() => settle(failed), 10_000);
    // Settling after unmount only clears the slot and timer.
    cancel.current = settle;
    setOpening(true);
    setError("");
    store.setState({ terminalHandoff: handoff });
    // The Agent tools seam opens the dock; the dock takes the request.
    tools.launch("terminal", chatId);
  };
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
          onClick={openInTerminal}
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

import {
  useCallback,
  useLayoutEffect,
  useRef,
  useState,
  type Dispatch,
  type KeyboardEvent,
  type RefObject,
} from "react";
import "../../styles/composer-slash.css";
import { ComposerCommandHint } from "./ComposerCommandHint";
import { ComposerSlashMenu } from "./ComposerSlashMenu";
import {
  pendingSlashCommandHint,
  slashCommandName,
  slashCommandQuery,
  slashTokenEnd,
  type PickedSlashCommand,
} from "./composerSlashCommand";

/** What the composer's own handlers ask of the loaded controller. */
export interface SlashControl {
  /** True when the open menu took the key. */
  onKeyDown(event: KeyboardEvent<HTMLTextAreaElement>): boolean;
  trackCaret(field: HTMLTextAreaElement): void;
  setFocused(focused: boolean): void;
  /** The command the user picked, while the prompt still names it. */
  pickedCommandFor(prompt: string): string | undefined;
}

/** The field attributes the composer renders for the menu. */
export interface SlashFieldShare {
  "aria-autocomplete"?: "list";
  "aria-expanded"?: boolean;
  "aria-controls"?: string;
  "aria-activedescendant"?: string;
  /** The picked command's hint, for the field's description. */
  describedBy?: string;
}

/**
 * Replaces the leading `/token` with `/name ` through the editing commands,
 * so Undo restores what was typed, and leaves the caret after the space.
 */
function insertCommand(field: HTMLTextAreaElement, name: string) {
  const token = slashTokenEnd(field.value);
  // Keep the whitespace that already follows the name, or add a space.
  const end = token < field.value.length ? token + 1 : token;
  const replacement = `/${name}${field.value.charAt(token) || " "}`;
  field.focus();
  field.setSelectionRange(0, end);
  const inserted =
    typeof document.execCommand === "function" &&
    document.execCommand("insertText", false, replacement);
  if (!inserted) {
    field.setRangeText(replacement, 0, end, "end");
    field.dispatchEvent(new Event("input", { bubbles: true }));
  }
}

const plainEnter = (event: KeyboardEvent) =>
  event.key === "Enter" &&
  !event.shiftKey &&
  !event.altKey &&
  !event.ctrlKey &&
  !event.metaKey &&
  !event.nativeEvent.isComposing;

/**
 * The composer's `/` command menu controller. It loads with the field's
 * first focus, so the first-Chat path carries only a stub; the textarea
 * keeps the focus throughout and reports keys, caret and focus here.
 */
export function ComposerSlashController({
  anchorRef,
  fieldRef,
  boundChatId,
  controlRef,
  disabled,
  fieldId,
  onShare,
  onTextChange,
  text,
}: {
  anchorRef: RefObject<HTMLElement | null>;
  fieldRef: RefObject<HTMLTextAreaElement | null>;
  boundChatId?: string;
  controlRef: RefObject<SlashControl | null>;
  disabled: boolean;
  fieldId: string;
  onShare: Dispatch<SlashFieldShare>;
  onTextChange: (text: string) => void;
  text: string;
}) {
  const keyHandlerRef = useRef<((event: KeyboardEvent) => boolean) | null>(
    null,
  );
  const [scope, setScope] = useState(boundChatId);
  const [dismissed, setDismissed] = useState(false);
  // The menu belongs to the field: it hides while the focus is elsewhere.
  const [focused, setFocused] = useState(false);
  const [caret, setCaret] = useState<number | null>(null);
  const [activeOptionId, setActiveOptionId] = useState<string | null>(null);
  const [picked, setPicked] = useState<PickedSlashCommand | null>(null);
  const [status, setStatus] = useState("");
  // Another Chat starts with no pick and no dismissal.
  if (scope !== boundChatId) {
    setScope(boundChatId);
    setDismissed(false);
    setPicked(null);
  }
  const query = slashCommandQuery(text, caret);
  // Escape closes the menu until the caret leaves the command name.
  if (query === null && dismissed) setDismissed(false);
  if (!text && picked) setPicked(null);
  const open = query !== null && !dismissed && focused && !disabled;
  const listboxId = `${fieldId}-commands`;
  const hint = pendingSlashCommandHint(text, picked);
  const hintId = `${fieldId}-command-hint`;
  const dismiss = useCallback(() => setDismissed(true), []);
  const pick = useCallback(
    (command: PickedSlashCommand) => {
      setPicked(command);
      const field = fieldRef.current;
      if (field) insertCommand(field, command.name);
      else onTextChange(`/${command.name} `);
    },
    [fieldRef, onTextChange],
  );
  // The field may already hold the focus and a caret when this loads.
  useLayoutEffect(() => {
    const field = fieldRef.current;
    if (!field || document.activeElement !== field) return;
    setFocused(true);
    setCaret(field.selectionStart);
  }, [fieldRef]);
  useLayoutEffect(() => {
    controlRef.current = {
      onKeyDown: (event) => {
        if (!open) return false;
        const handler = keyHandlerRef.current;
        if (handler) return handler(event);
        // Before the list has rendered, Enter must not send a half-typed name.
        if (!plainEnter(event)) return false;
        event.preventDefault();
        return true;
      },
      trackCaret: (field) => setCaret(field.selectionStart),
      setFocused,
      pickedCommandFor: (prompt) =>
        picked && slashCommandName(prompt) === picked.name
          ? picked.name
          : undefined,
    };
  }, [controlRef, open, picked]);
  useLayoutEffect(
    () => () => {
      controlRef.current = null;
    },
    [controlRef],
  );
  const autocomplete = query !== null;
  const activeDescendant = open && activeOptionId ? activeOptionId : undefined;
  useLayoutEffect(() => {
    onShare({
      "aria-autocomplete": autocomplete ? "list" : undefined,
      "aria-expanded": open,
      "aria-controls": open ? listboxId : undefined,
      "aria-activedescendant": activeDescendant,
      describedBy: hint ? hintId : undefined,
    });
  }, [activeDescendant, autocomplete, hint, hintId, listboxId, onShare, open]);
  return (
    <>
      <ComposerCommandHint hint={hint} hintId={hintId} text={text} />
      {open ? (
        <ComposerSlashMenu
          anchorRef={anchorRef}
          boundChatId={boundChatId}
          keyHandlerRef={keyHandlerRef}
          listboxId={listboxId}
          onActiveOptionChange={setActiveOptionId}
          onStatusChange={setStatus}
          onDismiss={dismiss}
          onPick={pick}
          query={query}
        />
      ) : null}
      {/* Mounted with the usable field, ahead of the menu, so its first
          announcement is not lost. */}
      {disabled ? null : (
        <span className="sr-only" role="status" aria-live="polite">
          {open ? status : ""}
        </span>
      )}
    </>
  );
}

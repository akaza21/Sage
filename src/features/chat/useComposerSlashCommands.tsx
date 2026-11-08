import {
  useCallback,
  useRef,
  useState,
  type KeyboardEvent,
  type RefObject,
} from "react";
import type * as SlashMenuChunk from "./ComposerSlashMenu";
import {
  pendingSlashCommandHint,
  slashCommandName,
  slashCommandQuery,
  slashTokenEnd,
  type PickedSlashCommand,
} from "./composerSlashCommand";
import { lazyParts } from "./lazyParts";

const SlashMenuPart = lazyParts<
  Pick<typeof SlashMenuChunk, "ComposerSlashMenu">
>(() => import("./ComposerSlashMenu"));

const plainEnter = (event: KeyboardEvent) =>
  event.key === "Enter" &&
  !event.shiftKey &&
  !event.altKey &&
  !event.ctrlKey &&
  !event.metaKey &&
  !event.nativeEvent.isComposing;

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

/**
 * The composer's `/` command menu. Only this small controller is on the
 * first-Chat path; the menu, its native list and its styles load with the
 * first `/` (or the first focus). The textarea keeps the focus throughout.
 */
export function useComposerSlashCommands({
  anchorRef,
  fieldRef,
  boundChatId,
  disabled,
  fieldId,
  text,
  onTextChange,
}: {
  anchorRef: RefObject<HTMLElement | null>;
  fieldRef: RefObject<HTMLTextAreaElement | null>;
  boundChatId?: string;
  disabled: boolean;
  fieldId: string;
  text: string;
  onTextChange: (text: string) => void;
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
  /** True when the open menu took the key. */
  const onKeyDown = useCallback(
    (event: KeyboardEvent) => {
      if (!open) return false;
      const handler = keyHandlerRef.current;
      if (handler) return handler(event);
      // Before the menu has loaded, Enter must not send a half-typed name.
      if (!plainEnter(event)) return false;
      event.preventDefault();
      return true;
    },
    [open],
  );
  const trackCaret = useCallback(
    (event: { currentTarget: HTMLTextAreaElement }) =>
      setCaret(event.currentTarget.selectionStart),
    [],
  );
  return {
    hint: pendingSlashCommandHint(text, picked),
    hintId: `${fieldId}-command-hint`,
    status: open ? status : "",
    fieldProps: {
      "aria-autocomplete": query !== null ? ("list" as const) : undefined,
      "aria-expanded": open,
      "aria-controls": open ? listboxId : undefined,
      "aria-activedescendant":
        open && activeOptionId ? activeOptionId : undefined,
      onSelect: trackCaret,
    },
    trackCaret,
    onFocus: () => {
      setFocused(true);
      void SlashMenuPart.preload().catch(() => undefined);
    },
    onBlur: () => setFocused(false),
    onKeyDown,
    /** The command the user picked, while the prompt still names it. */
    pickedCommandFor: (prompt: string) =>
      picked && slashCommandName(prompt) === picked.name
        ? picked.name
        : undefined,
    menu: open ? (
      <SlashMenuPart
        part="ComposerSlashMenu"
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
    ) : null,
  };
}

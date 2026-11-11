import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type RefObject,
} from "react";
import "../../styles/composer-slash.css";
import { isImeComposing } from "../../ui/imeComposition";
import { useOwnedFieldAttributes } from "../../ui/ownedFieldAttributes";
import type { SuggestionKeyEvent } from "../../ui/textFieldSuggestionNavigation";
import { ComposerCommandHint } from "./ComposerCommandHint";
import type * as MenuChunk from "./ComposerSlashMenu";
import { lazyParts } from "./lazyParts";
import {
  pendingSlashCommandHint,
  slashCommandName,
  slashCommandQuery,
  slashTokenEnd,
  type PickedSlashCommand,
} from "./composerSlashCommand";
import { setPickedCommand } from "./slashCommandPicks";

// The menu, its list and the command port load with the first `/`, so a
// focus that never types one carries only the key guard and the hint.
const MenuPart = lazyParts<typeof MenuChunk>(
  () => import("./ComposerSlashMenu"),
);

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

const plainEnter = (event: SuggestionKeyEvent) =>
  event.key === "Enter" &&
  !event.shiftKey &&
  !event.altKey &&
  !event.ctrlKey &&
  !event.metaKey &&
  !isImeComposing(event);

const CARET_EVENTS = ["input", "change", "select", "keyup", "mouseup"] as const;

/** Sets or clears one attribute the composer leaves to this controller. */
function setAttribute(field: HTMLElement, name: string, value: string | null) {
  if (value === null) field.removeAttribute(name);
  else field.setAttribute(name, value);
}

/**
 * The composer's `/` command menu. The list loads with the first `/` typed,
 * so the first-Chat path carries only the mount and an Enter guard. It
 * listens on the textarea itself (which keeps the focus throughout), takes
 * the keys the open menu uses before the composer's own handler sees them,
 * and holds the field's combobox attributes while its command name is edited.
 */
export function ComposerSlashController({
  fieldRef,
  boundChatId,
  disabled,
  fieldId,
  onTextChange,
  readyRef,
  text,
}: {
  fieldRef: RefObject<HTMLTextAreaElement | null>;
  boundChatId?: string;
  disabled: boolean;
  fieldId: string;
  onTextChange: (text: string) => void;
  /** Set while this controller handles the field's keys. */
  readyRef: RefObject<boolean>;
  text: string;
}) {
  const keyHandlerRef = useRef<((event: SuggestionKeyEvent) => boolean) | null>(
    null,
  );
  const anchorRef = useRef<HTMLElement | null>(null);
  const [scope, setScope] = useState(boundChatId);
  const [dismissed, setDismissed] = useState(false);
  // The menu belongs to the field: it hides while the focus is elsewhere.
  const [focused, setFocused] = useState(false);
  const [caret, setCaret] = useState<number | null>(null);
  const [activeOptionId, setActiveOptionId] = useState<string | null>(null);
  const [picked, setPicked] = useState<PickedSlashCommand | null>(null);
  const [status, setStatus] = useState("");
  // The field names the listbox only once the menu's chunk has mounted it.
  const [mounted, setMounted] = useState(false);
  // A menu chunk that failed to load gives way until the field is left or
  // the `/` is gone, so Enter sends and the next menu loads again.
  const [failed, setFailed] = useState(false);
  // Another Chat starts with no pick and no dismissal.
  if (scope !== boundChatId) {
    setScope(boundChatId);
    setDismissed(false);
    setPicked(null);
  }
  const query = slashCommandQuery(text, caret);
  // Escape closes the menu until the caret leaves the command name.
  if (query === null && dismissed) setDismissed(false);
  if (query === null && failed) setFailed(false);
  if (!text && picked) setPicked(null);
  const open = query !== null && !dismissed && !failed && focused && !disabled;
  const shown = open && mounted;
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
  // The send route reads the pick while the prompt still names it.
  const pickedName =
    picked && slashCommandName(text) === picked.name ? picked.name : null;
  useLayoutEffect(() => {
    if (!boundChatId) return;
    setPickedCommand(boundChatId, pickedName);
    return () => setPickedCommand(boundChatId, null);
  }, [boundChatId, pickedName]);
  const openRef = useRef(open);
  useLayoutEffect(() => {
    openRef.current = open;
  });
  useEffect(() => {
    const field = fieldRef.current;
    if (!field) return;
    anchorRef.current = field.parentElement;
    const track = () => setCaret(field.selectionStart);
    const focus = () => setFocused(true);
    // Leaving the field ends an Escape: the menu opens again on return.
    const blur = () => {
      setFocused(false);
      setDismissed(false);
      setFailed(false);
    };
    // Runs on the field before the composer's delegated handler, so a key
    // the open menu takes never sends or reaches the composer.
    const keyDown = (event: KeyboardEvent) => {
      if (!openRef.current) return;
      const handler = keyHandlerRef.current;
      const taken = handler
        ? handler(event)
        : // Before the list renders, Enter must not send a half-typed name.
          plainEnter(event) && (event.preventDefault(), true);
      if (taken) event.stopPropagation();
    };
    // The field may already hold the focus and a caret when this loads.
    if (document.activeElement === field) {
      focus();
      track();
    }
    for (const name of CARET_EVENTS) field.addEventListener(name, track);
    field.addEventListener("focus", focus);
    field.addEventListener("blur", blur);
    field.addEventListener("keydown", keyDown);
    readyRef.current = true;
    return () => {
      readyRef.current = false;
      for (const name of CARET_EVENTS) field.removeEventListener(name, track);
      field.removeEventListener("focus", focus);
      field.removeEventListener("blur", blur);
      field.removeEventListener("keydown", keyDown);
    };
  }, [fieldRef, readyRef]);
  // Only while the caret is in the command name: the rest of the time the
  // field's other owners (such as the `@` picker) keep these attributes,
  // and the composer's own aria-expanded="false" comes back.
  useOwnedFieldAttributes(fieldRef, query !== null, () => ({
    "aria-autocomplete": "list",
    "aria-expanded": String(shown),
    "aria-controls": shown ? listboxId : null,
    "aria-activedescendant": shown && activeOptionId ? activeOptionId : null,
  }));
  // The composer renders its own description list; this keeps the hint in
  // it after every render.
  useLayoutEffect(() => {
    const field = fieldRef.current;
    if (!field) return;
    const described = (field.getAttribute("aria-describedby") ?? "")
      .split(" ")
      .filter((id) => id && id !== hintId);
    if (hint) described.push(hintId);
    setAttribute(field, "aria-describedby", described.join(" ") || null);
  });
  return (
    <>
      <ComposerCommandHint hint={hint} hintId={hintId} text={text} />
      {open ? (
        <MenuPart
          part="ComposerSlashMenu"
          anchorRef={anchorRef}
          boundChatId={boundChatId}
          keyHandlerRef={keyHandlerRef}
          listboxId={listboxId}
          onActiveOptionChange={setActiveOptionId}
          onStatusChange={setStatus}
          onDismiss={dismiss}
          onLoadFailed={() => setFailed(true)}
          onMountedChange={setMounted}
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

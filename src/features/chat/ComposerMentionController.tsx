import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type RefObject,
} from "react";
import type { SuggestionKeyEvent } from "../../ui/textFieldSuggestionNavigation";
import { ComposerMentionMenu } from "./ComposerMentionMenu";
import { mentionQuery, mentionToken } from "./composerMention";

/**
 * Replaces the `@` token at [start, end) with the picked file's token
 * through the editing commands, so Undo restores what was typed, and leaves
 * the caret after the whitespace that follows it.
 */
function insertMention(
  field: HTMLTextAreaElement,
  start: number,
  end: number,
  relativePath: string,
) {
  const value = field.value;
  // Keep the whitespace that already follows the token, or add a space.
  const through = end < value.length ? end + 1 : end;
  const replacement = `${mentionToken(relativePath)}${value.charAt(end) || " "}`;
  field.focus();
  field.setSelectionRange(start, through);
  const inserted =
    typeof document.execCommand === "function" &&
    document.execCommand("insertText", false, replacement);
  if (!inserted) {
    field.setRangeText(replacement, start, through, "end");
    field.dispatchEvent(new Event("input", { bubbles: true }));
  }
}

const CARET_EVENTS = ["input", "change", "select", "keyup", "mouseup"] as const;
/** The combobox attributes this controller sets while a token is edited. */
const OWNED = ["aria-autocomplete", "aria-controls", "aria-activedescendant"];

/**
 * The composer's `@` file picker. It loads with the first `@` in the draft,
 * so the first-Chat path carries only its mount. It listens on the textarea
 * itself (which keeps the focus), takes the keys the open list uses before
 * the composer's own handler sees them, and sets the field's list
 * attributes (WAI-ARIA combobox pattern) only while the caret is in an `@`
 * token.
 */
export function ComposerMentionController({
  fieldRef,
  disabled,
  fieldId,
  onTextChange,
  text,
}: {
  fieldRef: RefObject<HTMLTextAreaElement | null>;
  disabled: boolean;
  fieldId: string;
  onTextChange: (text: string) => void;
  text: string;
}) {
  const keyHandlerRef = useRef<((event: SuggestionKeyEvent) => boolean) | null>(
    null,
  );
  const anchorRef = useRef<HTMLElement | null>(null);
  const [dismissed, setDismissed] = useState<number | null>(null);
  // The list belongs to the field: it hides while the focus is elsewhere.
  const [focused, setFocused] = useState(false);
  const [caret, setCaret] = useState<number | null>(null);
  const [activeOptionId, setActiveOptionId] = useState<string | null>(null);
  const [status, setStatus] = useState("");
  const token = mentionQuery(text, caret);
  // Escape closes the list for that token only.
  if (dismissed !== null && token?.start !== dismissed) setDismissed(null);
  const open = token !== null && dismissed === null && focused && !disabled;
  const listboxId = `${fieldId}-mentions`;
  const tokenRef = useRef(token);
  const openRef = useRef(open);
  useLayoutEffect(() => {
    tokenRef.current = token;
    openRef.current = open;
  });
  const dismiss = useCallback(
    () => setDismissed(tokenRef.current?.start ?? null),
    [],
  );
  const pick = useCallback(
    (relativePath: string) => {
      const at = tokenRef.current;
      const field = fieldRef.current;
      if (!at) return;
      if (field) insertMention(field, at.start, at.end, relativePath);
      else
        onTextChange(
          `${text.slice(0, at.start)}${mentionToken(relativePath)} ${text.slice(at.end)}`,
        );
    },
    [fieldRef, onTextChange, text],
  );
  useEffect(() => {
    const field = fieldRef.current;
    if (!field) return;
    anchorRef.current = field.parentElement;
    const track = () => setCaret(field.selectionStart);
    const focus = () => setFocused(true);
    const blur = () => {
      setFocused(false);
      setDismissed(null);
    };
    // Runs on the field before the composer's delegated handler, so a key
    // the open list takes never sends or reaches the composer.
    const keyDown = (event: KeyboardEvent) => {
      if (!openRef.current) return;
      if (keyHandlerRef.current?.(event)) event.stopPropagation();
    };
    // The field already holds the focus and a caret when this loads.
    if (document.activeElement === field) {
      focus();
      track();
    }
    for (const name of CARET_EVENTS) field.addEventListener(name, track);
    field.addEventListener("focus", focus);
    field.addEventListener("blur", blur);
    field.addEventListener("keydown", keyDown);
    return () => {
      for (const name of CARET_EVENTS) field.removeEventListener(name, track);
      field.removeEventListener("focus", focus);
      field.removeEventListener("blur", blur);
      field.removeEventListener("keydown", keyDown);
    };
  }, [fieldRef]);
  // Only while the caret is in an `@` token: the rest of the time the
  // field's other owners (such as the `/` menu) keep these attributes.
  const owning = useRef(false);
  useLayoutEffect(() => {
    const field = fieldRef.current;
    if (!field || (!token && !owning.current)) return;
    owning.current = token !== null;
    const set = (name: string, value: string | null) =>
      value === null
        ? field.removeAttribute(name)
        : field.setAttribute(name, value);
    set("aria-autocomplete", token ? "list" : null);
    // aria-expanded belongs to a combobox field; a plain textbox points at
    // the list through aria-controls and aria-activedescendant alone.
    if (field.getAttribute("role") === "combobox")
      set("aria-expanded", String(open));
    set("aria-controls", open ? listboxId : null);
    set("aria-activedescendant", open ? activeOptionId : null);
  });
  useLayoutEffect(
    () => () => {
      const field = fieldRef.current;
      if (!field || !owning.current) return;
      for (const name of OWNED) field.removeAttribute(name);
      if (field.getAttribute("role") === "combobox")
        field.setAttribute("aria-expanded", "false");
    },
    [fieldRef],
  );
  return (
    <>
      {open && token ? (
        <ComposerMentionMenu
          anchorRef={anchorRef}
          keyHandlerRef={keyHandlerRef}
          listboxId={listboxId}
          onActiveOptionChange={setActiveOptionId}
          onStatusChange={setStatus}
          onDismiss={dismiss}
          onPick={pick}
          query={token.query}
        />
      ) : null}
      {/* Mounted with the usable field, ahead of the list, so its first
          announcement is not lost. */}
      {disabled ? null : (
        <span className="sr-only" role="status" aria-live="polite">
          {open ? status : ""}
        </span>
      )}
    </>
  );
}

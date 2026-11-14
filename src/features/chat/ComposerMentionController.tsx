import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type RefObject,
} from "react";
import { useOwnedFieldAttributes } from "../../ui/ownedFieldAttributes";
import type { SuggestionKeyEvent } from "../../ui/textFieldSuggestionNavigation";
import { ComposerMentionMenu } from "./ComposerMentionMenu";
import { mentionQuery } from "./composerMention";

// Goes through the editing commands so Undo restores what was typed in one step.
function insertMention(
  field: HTMLTextAreaElement,
  start: number,
  end: number,
  token: string,
) {
  // The character after the token is rewritten as itself to step over it.
  const follows = field.value.charAt(end);
  const through = follows ? end + 1 : end;
  const replacement = `${token}${follows || " "}`;
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

export function ComposerMentionController({
  fieldRef,
  disabled,
  fieldId,
  text,
}: {
  fieldRef: RefObject<HTMLTextAreaElement | null>;
  disabled: boolean;
  fieldId: string;
  text: string;
}) {
  const keyHandlerRef = useRef<((event: SuggestionKeyEvent) => boolean) | null>(
    null,
  );
  const anchorRef = useRef<HTMLElement | null>(null);
  const [dismissed, setDismissed] = useState<number | null>(null);
  const [focused, setFocused] = useState(false);
  const [caret, setCaret] = useState<number | null>(null);
  const [activeOptionId, setActiveOptionId] = useState<string | null>(null);
  const [status, setStatus] = useState("");
  const token = mentionQuery(text, caret);
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
    (token: string) => {
      const at = tokenRef.current;
      const field = fieldRef.current;
      if (at && field) insertMention(field, at.start, at.end, token);
    },
    [fieldRef],
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
  useOwnedFieldAttributes(fieldRef, token !== null, (field) => ({
    "aria-autocomplete": "list",
    // aria-expanded belongs to a combobox field; a plain textbox points at
    // the list through aria-controls and aria-activedescendant alone.
    ...(field.getAttribute("role") === "combobox"
      ? { "aria-expanded": String(open) }
      : {}),
    "aria-controls": open ? listboxId : null,
    "aria-activedescendant": open ? activeOptionId : null,
  }));
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

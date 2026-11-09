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
import type { SuggestionKeyEvent } from "../../ui/textFieldSuggestionNavigation";
import { ComposerCommandHint } from "./ComposerCommandHint";
import { ComposerSlashMenu } from "./ComposerSlashMenu";
import type { ComposerCommand } from "./composerCommands";
import {
  pendingSlashCommandHint,
  slashCommandName,
  slashCommandQuery,
  slashTokenEnd,
  type PickedSlashCommand,
} from "./composerSlashCommand";
import { setPickedCommand } from "./slashCommandPicks";

/**
 * Replaces the leading `/token` and the whitespace after it through the
 * editing commands, so Undo restores what was typed, and leaves the caret
 * after the replacement.
 */
function replaceCommandToken(field: HTMLTextAreaElement, replacement: string) {
  const token = slashTokenEnd(field.value);
  const end = token < field.value.length ? token + 1 : token;
  field.focus();
  field.setSelectionRange(0, end);
  const inserted =
    typeof document.execCommand === "function" &&
    document.execCommand(
      replacement ? "insertText" : "delete",
      false,
      replacement,
    );
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
 * The composer's `/` command menu. It loads with the field's first focus,
 * so the first-Chat path carries only the mount and an Enter guard. It
 * listens on the textarea itself (which keeps the focus throughout), takes
 * the keys the open menu uses before the composer's own handler sees them,
 * and keeps the field's combobox attributes.
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
  const [Panel, setPanel] = useState<ComposerCommand["panel"]>();
  // Another Chat starts with no pick, dismissal or panel.
  if (scope !== boundChatId) {
    setScope(boundChatId);
    setDismissed(false);
    setPicked(null);
    setPanel(undefined);
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
    (command: ComposerCommand) => {
      const field = fieldRef.current;
      // A harness command keeps the whitespace after its name, or adds a
      // space; the chat's own commands write their text in its place.
      const text =
        command.text ??
        `/${command.name}${field?.value.charAt(slashTokenEnd(field.value)) || " "}`;
      if (field) replaceCommandToken(field, text);
      else onTextChange(text);
      if (command.text === undefined) setPicked(command);
      command.run?.();
      setPanel(() => command.panel);
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
  // The composer renders aria-expanded="false" and its own description
  // list; this keeps the menu's attributes on the field after every render.
  useLayoutEffect(() => {
    const field = fieldRef.current;
    if (!field) return;
    setAttribute(field, "aria-autocomplete", query === null ? null : "list");
    setAttribute(field, "aria-expanded", String(open));
    setAttribute(field, "aria-controls", open ? listboxId : null);
    setAttribute(
      field,
      "aria-activedescendant",
      open && activeOptionId ? activeOptionId : null,
    );
    const described = (field.getAttribute("aria-describedby") ?? "")
      .split(" ")
      .filter((id) => id && id !== hintId);
    if (hint) described.push(hintId);
    setAttribute(field, "aria-describedby", described.join(" ") || null);
  });
  useLayoutEffect(
    () => () => {
      const field = fieldRef.current;
      if (!field) return;
      for (const name of ["aria-autocomplete", "aria-controls"])
        field.removeAttribute(name);
      field.removeAttribute("aria-activedescendant");
      field.setAttribute("aria-expanded", "false");
    },
    [fieldRef],
  );
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
      {Panel ? (
        <Panel
          boundChatId={boundChatId}
          fieldRef={fieldRef}
          onClose={() => setPanel(undefined)}
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

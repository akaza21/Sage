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
import type * as PanelChunk from "./ComposerCommandPanel";
import { lazyParts } from "./lazyParts";
import type { AcpCommandPanel } from "../../domain/acpCommand";
import {
  pendingSlashCommandHint,
  slashCommandName,
  slashCommandQuery,
  slashTokenEnd,
  type PickedSlashCommand,
} from "./composerSlashCommand";
import { setPickedCommand } from "./slashCommandPicks";

const MenuPart = lazyParts<typeof MenuChunk>(
  () => import("./ComposerSlashMenu"),
);
const PanelPart = lazyParts<typeof PanelChunk>(
  () => import("./ComposerCommandPanel"),
);

// Goes through the editing commands so Undo restores what was typed.
function insertCommand(field: HTMLTextAreaElement, name: string | null) {
  const token = slashTokenEnd(field.value);
  const end = token < field.value.length ? token + 1 : token;
  const replacement =
    name === null ? "" : `/${name}${field.value.charAt(token) || " "}`;
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

function setAttribute(field: HTMLElement, name: string, value: string | null) {
  if (value === null) field.removeAttribute(name);
  else field.setAttribute(name, value);
}

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
  readyRef: RefObject<boolean>;
  text: string;
}) {
  const keyHandlerRef = useRef<((event: SuggestionKeyEvent) => boolean) | null>(
    null,
  );
  const anchorRef = useRef<HTMLElement | null>(null);
  const [scope, setScope] = useState(boundChatId);
  const [dismissed, setDismissed] = useState(false);
  const [focused, setFocused] = useState(false);
  const [caret, setCaret] = useState<number | null>(null);
  const [activeOptionId, setActiveOptionId] = useState<string | null>(null);
  const [picked, setPicked] = useState<PickedSlashCommand | null>(null);
  const [panel, setPanel] = useState<AcpCommandPanel | null>(null);
  const [status, setStatus] = useState("");
  const [mounted, setMounted] = useState(false);
  const [failed, setFailed] = useState(false);
  if (scope !== boundChatId) {
    setScope(boundChatId);
    setDismissed(false);
    setPicked(null);
    setPanel(null);
  }
  const query = slashCommandQuery(text, caret);
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
      const field = fieldRef.current;
      if (command.panel) {
        setPanel(command.panel);
        if (field) insertCommand(field, null);
        else onTextChange("");
        return;
      }
      setPicked(command);
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
      {panel ? (
        <PanelPart
          part="ComposerCommandPanel"
          kind={panel}
          boundChatId={boundChatId}
          fieldRef={fieldRef}
          onClose={() => setPanel(null)}
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

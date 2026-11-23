import {
  insertSlashCommand,
  pickComposerSlashCommand,
  takeComposerSlashPick,
} from "./insertSlashCommand";
import {
  useSessionCommands,
  type SessionCommandsLoad,
} from "./useSessionCommands";
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type RefObject,
  type ReactNode,
} from "react";
import "../../styles/composer-slash.css";
import { isImeComposing } from "../../ui/imeComposition";
import { useOwnedFieldAttributes } from "../../ui/ownedFieldAttributes";
import type { SuggestionKeyEvent } from "../../ui/textFieldSuggestionNavigation";
import { ComposerCommandHint } from "./ComposerCommandHint";
import { ComposerTerminalCatch } from "./ComposerTerminalCatch";
import type * as MenuChunk from "./ComposerSlashMenu";
import type * as PanelChunk from "./ComposerCommandPanel";
import { lazyParts } from "./lazyParts";
import type { AcpCommandPanel } from "../../domain/acpCommand";
import {
  pendingSlashCommandHint,
  slashCommandName,
  slashCommandQuery,
  type PickedSlashCommand,
} from "./composerSlashCommand";
import { setPickedCommand } from "./slashCommandPicks";

// Menus and typed sends share one lazy catalog entry and transport.
// eslint-disable-next-line react-refresh/only-export-components -- the chunk's command action, shared by its parts
export { slashCommandTurn, parseSlashCommand } from "./slashCommandTurn";
// For the facade's first-send hold.
// eslint-disable-next-line react-refresh/only-export-components -- the chunk's command port and picks
export { acpCommandsDesktop } from "../../services/acpCommandsDesktop";
// eslint-disable-next-line react-refresh/only-export-components -- see above
export { setPickedCommand };
// The new-chat draft's built-in rows insert through it, for Undo.
// eslint-disable-next-line react-refresh/only-export-components -- see above
export { insertSlashCommand };

const MenuPart = lazyParts<typeof MenuChunk>(
  () => import("./ComposerSlashMenu"),
);
const PanelPart = lazyParts<typeof PanelChunk>(
  () => import("./ComposerCommandPanel"),
);

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
  const [terminalStatus, setTerminalStatus] = useState("");
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
        if (field) insertSlashCommand(field, null);
        else onTextChange("");
        return;
      }
      setPicked(command);
      if (field) insertSlashCommand(field, command.name);
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
  useEffect(() => {
    const field = fieldRef.current;
    if (!field || disabled) return;
    const adoptPick = () => {
      const command = takeComposerSlashPick(field);
      if (command) setPicked(command);
    };
    adoptPick();
    field.addEventListener("sage-command-picked", adoptPick);
    return () => field.removeEventListener("sage-command-picked", adoptPick);
  }, [disabled, fieldRef]);
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
      {boundChatId ? (
        <ComposerTerminalCatch
          chatId={boundChatId}
          fieldRef={fieldRef}
          onAnnounce={setTerminalStatus}
          text={text}
        />
      ) : null}
      {/* Mounted with the usable field, ahead of the menu and any caught
          command, so its first announcement is not lost. */}
      {disabled ? null : (
        <span className="sr-only" role="status" aria-live="polite">
          {open ? status : terminalStatus}
        </span>
      )}
    </>
  );
}

export function ComposerCommandCatalog({
  boundChatId,
  children,
}: {
  boundChatId?: string;
  children: (
    load: SessionCommandsLoad | null,
    insertCommand: typeof pickComposerSlashCommand,
  ) => ReactNode;
}) {
  return children(useSessionCommands(boundChatId), pickComposerSlashCommand);
}

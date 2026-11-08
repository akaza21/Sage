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
  slashCommandQuery,
  slashCommandText,
  type PickedSlashCommand,
} from "./composerSlashCommand";
import { lazyParts } from "./lazyParts";

const SlashMenuPart = lazyParts<
  Pick<typeof SlashMenuChunk, "ComposerSlashMenu">
>(() => import("./ComposerSlashMenu"));

/**
 * The composer's `/` command menu. Only this small controller is on the
 * first-Chat path; the menu, its native list and its styles load with the
 * first `/`. The textarea keeps the focus throughout.
 */
export function useComposerSlashCommands({
  anchorRef,
  boundChatId,
  disabled,
  fieldId,
  text,
  onTextChange,
}: {
  anchorRef: RefObject<HTMLElement | null>;
  boundChatId?: string;
  disabled: boolean;
  fieldId: string;
  text: string;
  onTextChange: (text: string) => void;
}) {
  const keyHandlerRef = useRef<((event: KeyboardEvent) => boolean) | null>(
    null,
  );
  const [dismissed, setDismissed] = useState(false);
  // The menu belongs to the field: it hides while the focus is elsewhere.
  const [focused, setFocused] = useState(false);
  const [activeOptionId, setActiveOptionId] = useState<string | null>(null);
  const [picked, setPicked] = useState<PickedSlashCommand | null>(null);
  const query = slashCommandQuery(text);
  // Escape closes the menu until the composer leaves command-name text.
  if (query === null && dismissed) setDismissed(false);
  const open = query !== null && !dismissed && focused && !disabled;
  const listboxId = `${fieldId}-commands`;
  const dismiss = useCallback(() => setDismissed(true), []);
  const pick = useCallback(
    (command: PickedSlashCommand) => {
      setPicked(command);
      onTextChange(slashCommandText(command.name));
    },
    [onTextChange],
  );
  /** True when the open menu took the key. */
  const onKeyDown = useCallback(
    (event: KeyboardEvent) => open && (keyHandlerRef.current?.(event) ?? false),
    [open],
  );
  return {
    hint: pendingSlashCommandHint(text, picked),
    hintId: `${fieldId}-command-hint`,
    fieldProps: {
      "aria-autocomplete": "list" as const,
      "aria-controls": open ? listboxId : undefined,
      "aria-activedescendant":
        open && activeOptionId ? activeOptionId : undefined,
    },
    onFocus: () => setFocused(true),
    onBlur: () => setFocused(false),
    onKeyDown,
    menu: open ? (
      <SlashMenuPart
        part="ComposerSlashMenu"
        anchorRef={anchorRef}
        boundChatId={boundChatId}
        keyHandlerRef={keyHandlerRef}
        listboxId={listboxId}
        onActiveOptionChange={setActiveOptionId}
        onDismiss={dismiss}
        onPick={pick}
        query={query}
      />
    ) : null,
  };
}

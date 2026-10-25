export type KeymapModifier = "forbidden" | "allowed" | "required";

export type KeymapScope = "Global" | "Agent" | "Agent chat" | "Editor";

export interface KeymapBinding {
  id: string;
  label: string;
  description: string;
  scope: KeymapScope;
  key: string;
  ctrlOrMeta: KeymapModifier;
  shift: KeymapModifier;
  alt: KeymapModifier;
  /**
   * On macOS the chord uses Control instead of Command, as Go to Line does in
   * native Mac editors (Command+G is Find Next there).
   */
  appleControl?: true;
  /** Shift and Alt on macOS when they differ, as Fold is Cmd+Option+[ there. */
  apple?: { shift: KeymapModifier; alt: KeymapModifier };
  /**
   * The ticket that makes this binding work. Until it lands the entry only
   * reserves the chord: nothing listens for it and the Keyboard page leaves
   * it out, so the page never lists a shortcut that does nothing.
   */
  reservedFor?: `EP-${number}`;
}

export interface KeymapChordEvent {
  key: string;
  altKey: boolean;
  ctrlKey: boolean;
  metaKey: boolean;
  shiftKey: boolean;
}

export const KEYMAP_BINDINGS: readonly KeymapBinding[] = Object.freeze([
  {
    id: "newTask",
    label: "New task",
    description: "Starts a new chat and switches to Agents.",
    scope: "Global",
    key: "n",
    ctrlOrMeta: "required",
    shift: "forbidden",
    alt: "forbidden",
  },
  {
    id: "openSettings",
    label: "Open settings",
    description: "Opens the Settings tab and switches to Editor.",
    scope: "Global",
    key: ",",
    ctrlOrMeta: "required",
    shift: "allowed",
    alt: "allowed",
  },
  {
    id: "chatSearch",
    label: "Search chats",
    description: "Focuses the title bar chat search.",
    scope: "Agent",
    key: "k",
    ctrlOrMeta: "required",
    shift: "forbidden",
    alt: "forbidden",
  },
  {
    id: "chat.findInChat",
    label: "Find in chat",
    description: "Search the visible chat's messages.",
    scope: "Agent chat",
    key: "f",
    ctrlOrMeta: "required",
    shift: "forbidden",
    alt: "forbidden",
  },
  {
    id: "focusComposer",
    label: "Focus composer",
    description: "Moves the caret into the message composer.",
    scope: "Agent",
    key: "l",
    ctrlOrMeta: "required",
    shift: "forbidden",
    alt: "forbidden",
  },
  {
    id: "sendMessage",
    label: "Send message",
    description: "Sends the composer contents to the agent.",
    scope: "Agent",
    key: "enter",
    ctrlOrMeta: "allowed",
    shift: "forbidden",
    alt: "forbidden",
  },
  {
    id: "newline",
    label: "Insert a line break",
    description: "Adds a new line in the composer instead of sending.",
    scope: "Agent",
    key: "enter",
    ctrlOrMeta: "forbidden",
    shift: "required",
    alt: "forbidden",
  },
  {
    id: "save",
    label: "Save file",
    description: "Writes the active editor buffer to disk.",
    scope: "Editor",
    key: "s",
    ctrlOrMeta: "required",
    shift: "allowed",
    alt: "forbidden",
  },
  {
    id: "find",
    label: "Find in file",
    description: "Opens the find bar for the active file.",
    scope: "Editor",
    key: "f",
    ctrlOrMeta: "required",
    shift: "forbidden",
    alt: "forbidden",
  },
  {
    id: "goToLine",
    label: "Go to line",
    description: "Jumps to a line number in the active file.",
    scope: "Editor",
    key: "g",
    ctrlOrMeta: "required",
    shift: "forbidden",
    alt: "forbidden",
    appleControl: true,
  },
  {
    id: "quickOpen",
    label: "Quick Open",
    description: "Opens a file in the project by typing part of its name.",
    scope: "Editor",
    key: "p",
    ctrlOrMeta: "required",
    shift: "forbidden",
    alt: "forbidden",
    reservedFor: "EP-08",
  },
  {
    id: "goToSymbol",
    label: "Go to symbol",
    description: "Jumps to a symbol defined in the active file.",
    scope: "Editor",
    key: "f12",
    ctrlOrMeta: "forbidden",
    shift: "forbidden",
    alt: "forbidden",
  },
  {
    id: "dismiss",
    label: "Dismiss",
    description: "Closes the preview, go to line, or find bar.",
    scope: "Editor",
    key: "escape",
    ctrlOrMeta: "forbidden",
    shift: "forbidden",
    alt: "forbidden",
  },
  {
    id: "commit",
    label: "Commit staged changes",
    description: "Submits the commit message in Source Control.",
    scope: "Editor",
    key: "enter",
    ctrlOrMeta: "required",
    shift: "forbidden",
    alt: "forbidden",
  },
]);

export function keymapBinding(id: string): KeymapBinding {
  const binding = KEYMAP_BINDINGS.find((entry) => entry.id === id);
  if (!binding) throw new Error(`unknown keymap binding: ${id}`);
  return binding;
}

function admits(state: KeymapModifier, pressed: boolean): boolean {
  if (state === "forbidden") return !pressed;
  if (state === "required") return pressed;
  return true;
}

export function matchesKeymapChord(
  binding: KeymapBinding,
  event: KeymapChordEvent,
): boolean {
  return (
    admits(binding.ctrlOrMeta, event.ctrlKey || event.metaKey) &&
    admits(binding.shift, event.shiftKey) &&
    admits(binding.alt, event.altKey) &&
    event.key.toLowerCase() === binding.key
  );
}

/**
 * Matches the platform's own modifier only: Command on macOS (Control there
 * is the text-editing modifier, as in Control+F for forward a character) and
 * Control elsewhere (the Windows key is the system's). `matchesKeymapChord`
 * keeps accepting either for surfaces that have not opted in yet.
 */
export function matchesPlatformChord(
  binding: KeymapBinding,
  event: KeymapChordEvent,
  apple: boolean,
): boolean {
  const useControl = !apple || binding.appleControl === true;
  const primary = useControl ? event.ctrlKey : event.metaKey;
  const other = useControl ? event.metaKey : event.ctrlKey;
  const { shift, alt } = (apple && binding.apple) || binding;
  return (
    !other &&
    admits(binding.ctrlOrMeta, primary) &&
    admits(shift, event.shiftKey) &&
    admits(alt, event.altKey) &&
    event.key.toLowerCase() === binding.key
  );
}

const KEY_LABELS: Record<string, string> = {
  escape: "Esc",
  f12: "F12",
  enter: "Enter",
  tab: "Tab",
  arrowup: "Up",
  arrowdown: "Down",
};

export function formatKeymapChords(
  binding: KeymapBinding,
  apple: boolean,
): string[] {
  const key =
    KEY_LABELS[binding.key] ??
    (binding.key === "," ? "," : binding.key.toUpperCase());
  const { shift, alt } = (apple && binding.apple) || binding;
  const chord = (withCtrlOrMeta: boolean) => {
    const parts: string[] = [];
    if (withCtrlOrMeta) {
      parts.push(apple && binding.appleControl !== true ? "Cmd" : "Ctrl");
    }
    if (shift === "required") parts.push("Shift");
    if (alt === "required") parts.push(apple ? "Option" : "Alt");
    parts.push(key);
    return parts.join("+");
  };
  if (binding.ctrlOrMeta === "allowed") return [chord(false), chord(true)];
  return [chord(binding.ctrlOrMeta === "required")];
}

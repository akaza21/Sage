export type KeymapModifier = "forbidden" | "allowed" | "required";

export type KeymapScope = "Global" | "Agent" | "Agent chat" | "Editor";

export interface KeymapBinding {
  id: string;
  label: string;
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
  apple?: {
    shift: KeymapModifier;
    alt: KeymapModifier;
    key?: string;
    ctrlOrMeta?: KeymapModifier;
  };
  /**
   * The ticket that makes this binding work. Until it lands the entry only
   * reserves the chord: nothing listens for it and the Keyboard page leaves
   * it out, so the page never lists a shortcut that does nothing.
   */
  reservedFor?: `EP-${number}`;
}

export interface KeymapChordEvent {
  key: string;
  /** The physical key, for Option chords on macOS. */
  code?: string;
  altKey: boolean;
  ctrlKey: boolean;
  metaKey: boolean;
  shiftKey: boolean;
  /** Reports AltGr, which Windows sends as Control+Alt. */
  getModifierState?: (key: "AltGraph") => boolean;
}

export const KEYMAP_BINDINGS: readonly KeymapBinding[] = Object.freeze([
  ...(
    [
      ["agent.changes", "Changes", "g", "required", "forbidden"],
      ["agent.files", "Files", "p", "forbidden", "forbidden"],
      ["agent.sideChat", "Side chat", "s", "forbidden", "required"],
    ] as const
  ).map(([id, label, key, shift, alt]) => ({
    id,
    label,
    key,
    shift,
    alt,
    scope: "Agent" as const,
    ctrlOrMeta: "required" as const,
  })),
  {
    id: "agent.terminal",
    label: "Toggle terminal",
    scope: "Global",
    key: "`",
    ctrlOrMeta: "required",
    shift: "forbidden",
    alt: "forbidden",
  },
  {
    id: "newTask",
    label: "New chat",
    scope: "Global",
    key: "n",
    ctrlOrMeta: "required",
    shift: "forbidden",
    alt: "forbidden",
  },
  {
    id: "toggleSidebar",
    label: "Toggle sidebar",
    scope: "Global",
    key: "b",
    ctrlOrMeta: "required",
    shift: "forbidden",
    alt: "forbidden",
  },
  {
    id: "openSettings",
    label: "Open settings",
    scope: "Global",
    key: ",",
    ctrlOrMeta: "required",
    shift: "allowed",
    alt: "allowed",
  },
  {
    id: "chatSearch",
    label: "Search chats",
    scope: "Agent",
    key: "k",
    ctrlOrMeta: "required",
    shift: "forbidden",
    alt: "forbidden",
  },
  {
    id: "chat.findInChat",
    label: "Find in chat",
    scope: "Agent chat",
    key: "f",
    ctrlOrMeta: "required",
    shift: "forbidden",
    alt: "forbidden",
  },
  {
    id: "focusComposer",
    label: "Focus composer",
    scope: "Agent",
    key: "l",
    ctrlOrMeta: "required",
    shift: "forbidden",
    alt: "forbidden",
  },
  {
    id: "sendMessage",
    label: "Send message",
    scope: "Agent",
    key: "enter",
    ctrlOrMeta: "allowed",
    shift: "forbidden",
    alt: "forbidden",
  },
  {
    id: "newline",
    label: "Insert a line break",
    scope: "Agent",
    key: "enter",
    ctrlOrMeta: "forbidden",
    shift: "required",
    alt: "forbidden",
  },
  {
    id: "save",
    label: "Save file",
    scope: "Editor",
    key: "s",
    ctrlOrMeta: "required",
    shift: "allowed",
    alt: "forbidden",
  },
  {
    // VS Code's Save All is the two-step Ctrl+K S; this keymap has no
    // two-step chords, so it takes VS Code's macOS chord on every platform.
    id: "saveAll",
    label: "Save all files",
    scope: "Editor",
    key: "s",
    ctrlOrMeta: "required",
    shift: "forbidden",
    alt: "required",
  },
  {
    id: "closeEditor",
    label: "Close editor",
    scope: "Editor",
    key: "w",
    ctrlOrMeta: "required",
    shift: "forbidden",
    alt: "forbidden",
  },
  {
    id: "find",
    label: "Find in file",
    scope: "Editor",
    key: "f",
    ctrlOrMeta: "required",
    shift: "forbidden",
    alt: "forbidden",
  },
  {
    id: "replace",
    label: "Replace in file",
    scope: "Editor",
    key: "h",
    ctrlOrMeta: "required",
    shift: "forbidden",
    alt: "forbidden",
    apple: { key: "f", shift: "forbidden", alt: "required" },
  },
  {
    id: "findNext",
    label: "Find next",
    scope: "Editor",
    key: "f3",
    ctrlOrMeta: "forbidden",
    shift: "forbidden",
    alt: "forbidden",
    apple: {
      key: "g",
      ctrlOrMeta: "required",
      shift: "forbidden",
      alt: "forbidden",
    },
  },
  {
    id: "findPrevious",
    label: "Find previous",
    scope: "Editor",
    key: "f3",
    ctrlOrMeta: "forbidden",
    shift: "required",
    alt: "forbidden",
    apple: {
      key: "g",
      ctrlOrMeta: "required",
      shift: "required",
      alt: "forbidden",
    },
  },
  {
    id: "goToLine",
    label: "Go to line",
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
    scope: "Editor",
    key: "p",
    ctrlOrMeta: "required",
    shift: "forbidden",
    alt: "forbidden",
  },
  {
    id: "commandPalette",
    label: "Show all commands",
    scope: "Editor",
    key: "p",
    ctrlOrMeta: "required",
    shift: "required",
    alt: "forbidden",
  },
  {
    id: "navigateBack",
    label: "Go back",
    scope: "Global",
    key: "arrowleft",
    ctrlOrMeta: "forbidden",
    shift: "forbidden",
    alt: "required",
    appleControl: true,
    apple: {
      key: "-",
      ctrlOrMeta: "required",
      shift: "forbidden",
      alt: "forbidden",
    },
  },
  {
    id: "navigateForward",
    label: "Go forward",
    scope: "Global",
    key: "arrowright",
    ctrlOrMeta: "forbidden",
    shift: "forbidden",
    alt: "required",
    appleControl: true,
    apple: {
      key: "-",
      ctrlOrMeta: "required",
      shift: "required",
      alt: "forbidden",
    },
  },
  {
    id: "goToSymbol",
    label: "Go to symbol",
    scope: "Editor",
    key: "f12",
    ctrlOrMeta: "forbidden",
    shift: "forbidden",
    alt: "forbidden",
  },
  {
    id: "dismiss",
    label: "Dismiss",
    scope: "Editor",
    key: "escape",
    ctrlOrMeta: "forbidden",
    shift: "forbidden",
    alt: "forbidden",
  },
  {
    id: "commit",
    label: "Commit staged changes",
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

function platformChord(binding: KeymapBinding, apple: boolean) {
  return { ...binding, ...(apple ? binding.apple : undefined) };
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
  const chord = platformChord(binding, apple);
  // AltGr types characters (AltGr+S is "ś" on a Polish layout); on Windows it
  // arrives as Control+Alt, which must not run a chord.
  return (
    !other &&
    !(!apple && event.getModifierState?.("AltGraph")) &&
    admits(chord.ctrlOrMeta, primary) &&
    admits(chord.shift, event.shiftKey) &&
    admits(chord.alt, event.altKey) &&
    (event.key.toLowerCase() === chord.key ||
      // Shift turns "-" into "_" on most layouts.
      (chord.key === "-" && event.shiftKey && event.key === "_") ||
      (apple && event.altKey && optionLetter(event.code) === chord.key))
  );
}

/**
 * Option turns Command+Option+F into "ƒ" on macOS, so an Option chord on a
 * letter matches the physical key instead.
 */
function optionLetter(code: string | undefined): string | undefined {
  return code && /^Key[A-Z]$/u.test(code)
    ? code.slice(3).toLowerCase()
    : undefined;
}

const KEY_LABELS: Record<string, string> = {
  escape: "Esc",
  f3: "F3",
  f12: "F12",
  enter: "Enter",
  tab: "Tab",
  arrowup: "Up",
  arrowdown: "Down",
  arrowleft: "Left",
  arrowright: "Right",
};

export function formatKeymapChords(
  binding: KeymapBinding,
  apple: boolean,
): string[] {
  const { shift, alt, ctrlOrMeta, ...chord } = platformChord(binding, apple);
  const key =
    KEY_LABELS[chord.key] ??
    (chord.key === "," ? "," : chord.key.toUpperCase());
  const format = (withCtrlOrMeta: boolean) => {
    const parts: string[] = [];
    if (withCtrlOrMeta) {
      parts.push(apple && binding.appleControl !== true ? "Cmd" : "Ctrl");
    }
    if (shift === "required") parts.push("Shift");
    if (alt === "required") parts.push(apple ? "Option" : "Alt");
    parts.push(key);
    return parts.join("+");
  };
  if (ctrlOrMeta === "allowed") return [format(false), format(true)];
  return [format(ctrlOrMeta === "required")];
}

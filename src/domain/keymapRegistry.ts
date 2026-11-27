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

const MODIFIERS = {
  f: "forbidden",
  a: "allowed",
  r: "required",
} as const;

type ModifierCode = keyof typeof MODIFIERS;

/**
 * One binding. `modifiers` spells Ctrl or Cmd, Shift and Alt in that order,
 * each as f (forbidden), a (allowed) or r (required): "rfa" requires Ctrl or
 * Cmd, forbids Shift and allows Alt.
 */
export function keymapChord<Extra extends object = object>(
  id: string,
  label: string,
  scope: KeymapScope,
  key: string,
  modifiers: `${ModifierCode}${ModifierCode}${ModifierCode}`,
  extra?: Extra,
): KeymapBinding & Extra {
  const [ctrlOrMeta, shift, alt] = [...modifiers].map(
    (code) => MODIFIERS[code as ModifierCode],
  );
  return {
    id,
    label,
    scope,
    key,
    ctrlOrMeta,
    shift,
    alt,
    ...extra,
  } as KeymapBinding & Extra;
}

export const KEYMAP_BINDINGS: readonly KeymapBinding[] = Object.freeze([
  keymapChord("agent.changes", "Changes", "Agent", "g", "rrf"),
  keymapChord("agent.files", "Files", "Agent", "p", "rff"),
  keymapChord("agent.sideChat", "Side chat", "Agent", "s", "rfr"),
  keymapChord("agent.terminal", "Toggle terminal", "Global", "`", "rff"),
  keymapChord("newTask", "New chat", "Global", "n", "rff"),
  keymapChord("toggleSidebar", "Toggle sidebar", "Global", "b", "rff"),
  keymapChord("openSettings", "Open settings", "Global", ",", "raa"),
  keymapChord("chatSearch", "Search chats", "Agent", "k", "rff"),
  keymapChord("chat.findInChat", "Find in chat", "Agent chat", "f", "rff"),
  keymapChord("focusComposer", "Focus composer", "Agent", "l", "rff"),
  keymapChord("sendMessage", "Send message", "Agent", "enter", "aff"),
  keymapChord("newline", "Insert a line break", "Agent", "enter", "frf"),
  keymapChord("save", "Save file", "Editor", "s", "raf"),
  // VS Code's Save All is the two-step Ctrl+K S; this keymap has no
  // two-step chords, so it takes VS Code's macOS chord on every platform.
  keymapChord("saveAll", "Save all files", "Editor", "s", "rfr"),
  keymapChord("closeEditor", "Close editor", "Editor", "w", "rff"),
  keymapChord("find", "Find in file", "Editor", "f", "rff"),
  keymapChord("replace", "Replace in file", "Editor", "h", "rff", {
    apple: { key: "f", shift: "forbidden", alt: "required" },
  }),
  keymapChord("findNext", "Find next", "Editor", "f3", "fff", {
    apple: {
      key: "g",
      ctrlOrMeta: "required",
      shift: "forbidden",
      alt: "forbidden",
    },
  }),
  keymapChord("findPrevious", "Find previous", "Editor", "f3", "frf", {
    apple: {
      key: "g",
      ctrlOrMeta: "required",
      shift: "required",
      alt: "forbidden",
    },
  }),
  keymapChord("goToLine", "Go to line", "Editor", "g", "rff", {
    appleControl: true,
  }),
  keymapChord("quickOpen", "Quick Open", "Editor", "p", "rff"),
  keymapChord("commandPalette", "Show all commands", "Editor", "p", "rrf"),
  keymapChord("navigateBack", "Go back", "Global", "arrowleft", "ffr", {
    appleControl: true,
    apple: {
      key: "-",
      ctrlOrMeta: "required",
      shift: "forbidden",
      alt: "forbidden",
    },
  }),
  keymapChord("navigateForward", "Go forward", "Global", "arrowright", "ffr", {
    appleControl: true,
    apple: {
      key: "-",
      ctrlOrMeta: "required",
      shift: "required",
      alt: "forbidden",
    },
  }),
  keymapChord("goToSymbol", "Go to symbol", "Editor", "f12", "fff"),
  keymapChord("dismiss", "Dismiss", "Editor", "escape", "fff"),
  keymapChord("commit", "Commit staged changes", "Editor", "enter", "rff"),
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

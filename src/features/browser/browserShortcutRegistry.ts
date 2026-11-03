export type BrowserShortcut =
  | "focusAddress"
  | "reload"
  | "back"
  | "forward"
  | "find"
  | "zoomIn"
  | "zoomOut"
  | "zoomReset"
  | "newTab"
  | "closeTab"
  | "stop";

interface BrowserShortcutBinding {
  action: BrowserShortcut;
  chord: string;
  keys: readonly string[];
  ctrl?: true;
  alt?: true;
  shift?: "allowed";
  shiftedCode?: string;
  loadingOnly?: true;
}

export const BROWSER_SHORTCUT_BINDINGS: readonly BrowserShortcutBinding[] = [
  { action: "focusAddress", chord: "Ctrl+L", keys: ["l"], ctrl: true },
  { action: "newTab", chord: "Ctrl+T", keys: ["t"], ctrl: true },
  { action: "closeTab", chord: "Ctrl+W", keys: ["w"], ctrl: true },
  { action: "reload", chord: "Ctrl+R", keys: ["r"], ctrl: true },
  { action: "reload", chord: "F5", keys: ["f5"] },
  { action: "find", chord: "Ctrl+F", keys: ["f"], ctrl: true },
  { action: "back", chord: "Alt+Left", keys: ["arrowleft"], alt: true },
  { action: "forward", chord: "Alt+Right", keys: ["arrowright"], alt: true },
  {
    action: "zoomIn",
    chord: "Ctrl++",
    keys: ["+", "=", "add"],
    ctrl: true,
    shift: "allowed",
    shiftedCode: "Equal",
  },
  { action: "zoomOut", chord: "Ctrl+-", keys: ["-"], ctrl: true },
  { action: "zoomReset", chord: "Ctrl+0", keys: ["0"], ctrl: true },
  { action: "stop", chord: "Esc", keys: ["escape"], loadingOnly: true },
];

export function browserShortcutFor(
  event: KeyboardEvent,
  loading: boolean,
): BrowserShortcut | undefined {
  if (event.metaKey) return;
  const key = event.key.toLowerCase();
  return BROWSER_SHORTCUT_BINDINGS.find(
    (binding) =>
      event.ctrlKey === Boolean(binding.ctrl) &&
      event.altKey === Boolean(binding.alt) &&
      (!event.shiftKey || binding.shift === "allowed") &&
      (!binding.loadingOnly || loading) &&
      (binding.keys.includes(key) || binding.shiftedCode === event.code),
  )?.action;
}

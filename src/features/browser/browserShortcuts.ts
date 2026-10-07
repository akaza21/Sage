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

export function dispatchBrowserChromeShortcut(
  root: HTMLElement,
  event: KeyboardEvent,
  loading: boolean,
  dispatch: (action: BrowserShortcut) => void,
): void {
  if (!(event.target instanceof Node) || !root.contains(event.target)) return;
  let action: BrowserShortcut | undefined;
  const key = event.key.toLowerCase();
  if (event.ctrlKey && !event.altKey && !event.metaKey) {
    if (!event.shiftKey) {
      action = (
        {
          l: "focusAddress",
          r: "reload",
          f: "find",
          t: "newTab",
          w: "closeTab",
          "0": "zoomReset",
          "-": "zoomOut",
        } as const
      )[key as "l"];
    }
    if (
      key === "+" ||
      key === "add" ||
      (event.code === "Equal" && event.shiftKey)
    ) {
      action = "zoomIn";
    }
  } else if (
    event.altKey &&
    !event.ctrlKey &&
    !event.shiftKey &&
    !event.metaKey
  ) {
    if (key === "arrowleft") action = "back";
    if (key === "arrowright") action = "forward";
  } else if (
    !event.ctrlKey &&
    !event.altKey &&
    !event.shiftKey &&
    !event.metaKey
  ) {
    if (key === "f5") action = "reload";
    if (key === "escape" && loading) action = "stop";
  }
  if (!action) return;
  event.preventDefault();
  event.stopPropagation();
  dispatch(action);
}

interface BrowserShortcutActions {
  address: HTMLInputElement | null;
  find: () => HTMLInputElement | null;
  control: (action: "back" | "forward" | "reload" | "stop") => void;
  findOpen: () => void;
  zoom: (delta: number) => void;
  zoomPercent: number;
  newTab: () => void;
  closeTab: () => void;
}

export function performBrowserShortcut(
  action: BrowserShortcut,
  actions: BrowserShortcutActions,
): void {
  switch (action) {
    case "focusAddress":
      actions.address?.focus();
      actions.address?.select();
      break;
    case "reload":
    case "back":
    case "forward":
    case "stop":
      actions.control(action);
      break;
    case "find":
      actions.findOpen();
      requestAnimationFrame(() => actions.find()?.focus());
      break;
    case "zoomIn":
      actions.zoom(25);
      break;
    case "zoomOut":
      actions.zoom(-25);
      break;
    case "zoomReset":
      actions.zoom(100 - actions.zoomPercent);
      break;
    case "newTab":
      actions.newTab();
      break;
    case "closeTab":
      actions.closeTab();
      break;
  }
}

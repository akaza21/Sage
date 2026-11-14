import {
  formatKeymapChords,
  keymapBinding,
  matchesPlatformChord,
} from "../../domain/keymapRegistry";
import { usesMetaModifier } from "../../ui/shortcutLabel";

export interface FileSurfaceCommandActions {
  previewing: boolean;
  findOpen: boolean;
  goToOpen: boolean;
  closePreview: () => void;
  closeFind: () => void;
  closeGoToLine: () => void;
  openFind: () => void;
  openReplace: () => void;
  moveFind: (direction: 1 | -1) => void;
  openGoToLine: () => void;
  goToLocalSymbol: () => void;
  save: () => void;
}

/** Replace is Command+Option+F on macOS because Command+H hides the app. */
export function handleFileSurfaceCommandKey(
  event: {
    key: string;
    code?: string;
    altKey: boolean;
    ctrlKey: boolean;
    metaKey: boolean;
    shiftKey: boolean;
    preventDefault: () => void;
  },
  actions: FileSurfaceCommandActions,
  apple: boolean = usesMetaModifier(),
): boolean {
  const matches = (id: string) =>
    matchesPlatformChord(keymapBinding(id), event, apple);
  if (matches("goToSymbol")) {
    event.preventDefault();
    if (!actions.previewing) actions.goToLocalSymbol();
    return true;
  }
  if (event.key === "Escape") {
    if (actions.previewing) {
      event.preventDefault();
      actions.closePreview();
      return true;
    }
    if (actions.goToOpen) {
      event.preventDefault();
      actions.closeGoToLine();
      return true;
    }
    if (actions.findOpen) {
      event.preventDefault();
      actions.closeFind();
      return true;
    }
    return false;
  }
  if (matches("save")) {
    event.preventDefault();
    actions.save();
    return true;
  }
  if (matches("find")) {
    event.preventDefault();
    if (!actions.previewing) actions.openFind();
    return true;
  }
  if (matches("replace")) {
    event.preventDefault();
    if (!actions.previewing) actions.openReplace();
    return true;
  }
  for (const [id, direction] of [
    ["findNext", 1],
    ["findPrevious", -1],
  ] as const) {
    if (!matches(id)) continue;
    event.preventDefault();
    if (!actions.previewing) actions.moveFind(direction);
    return true;
  }
  if (matches("goToLine")) {
    event.preventDefault();
    if (!actions.previewing) actions.openGoToLine();
    return true;
  }
  return false;
}

export function unsavedState(apple: boolean) {
  const [chord] = formatKeymapChords(keymapBinding("save"), apple);
  return {
    label: `Unsaved · ${chord}`,
    accessibleLabel: `Unsaved changes. Press ${apple ? "Command" : "Control"} plus S to save.`,
  };
}

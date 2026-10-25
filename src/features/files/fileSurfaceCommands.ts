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
  /** Steps through the find bar's matches, opening it when closed. */
  moveFind: (direction: 1 | -1) => void;
  openGoToLine: () => void;
  goToLocalSymbol: () => void;
  save: () => void;
}

/**
 * Editor chords from the keymap registry, matched with the platform's own
 * modifier: Command+S/F and Control+G on macOS, Control+S/F/G elsewhere.
 * Replace and Find next follow VS Code: Command+Option+F and Command+G on
 * macOS (Command+H hides the app), Control+H and F3 elsewhere.
 */
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
  // Escape dismisses whatever modifiers are held, as it always has.
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

/** Names the platform's save chord: Cmd+S on macOS, Ctrl+S elsewhere. */
export function unsavedState(apple: boolean) {
  const [chord] = formatKeymapChords(keymapBinding("save"), apple);
  return {
    label: `Unsaved · ${chord}`,
    accessibleLabel: `Unsaved changes. Press ${apple ? "Command" : "Control"} plus S to save.`,
  };
}

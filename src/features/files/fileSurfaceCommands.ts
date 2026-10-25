import {
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
  openGoToLine: () => void;
  goToLocalSymbol: () => void;
  save: () => void;
}

/**
 * Editor chords from the keymap registry, matched with the platform's own
 * modifier: Command+S/F and Control+G on macOS, Control+S/F/G elsewhere.
 */
export function handleFileSurfaceCommandKey(
  event: {
    key: string;
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
  if (matches("goToLine")) {
    event.preventDefault();
    if (!actions.previewing) actions.openGoToLine();
    return true;
  }
  return false;
}

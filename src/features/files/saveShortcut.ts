import { formatKeymapChords, keymapBinding } from "../../domain/keymapRegistry";
/** Names the platform's save chord: Cmd+S on macOS, Ctrl+S elsewhere. */
export function unsavedState(apple: boolean) {
  const [chord] = formatKeymapChords(keymapBinding("save"), apple);
  return {
    label: `Unsaved · ${chord}`,
    accessibleLabel: `Unsaved changes. Press ${apple ? "Command" : "Control"} plus S to save.`,
  };
}

import { useEffect } from "react";

import { keymapBinding, matchesPlatformChord } from "../domain/keymapRegistry";
import { usesMetaModifier } from "../ui/shortcutLabel";

// The history loads with the first File; until then there is nowhere to go.
function travel(forward: boolean) {
  void import("../app/editorHistory").then((history) =>
    forward ? history.goForward() : history.goBack(),
  );
}

/** A field other than the editor, where the chord edits text (a terminal). */
function typingElsewhere(target: EventTarget | null) {
  return (
    target instanceof Element &&
    !!target.closest("input, textarea, select, [contenteditable]") &&
    !target.closest(".cm-editor")
  );
}

/**
 * Go back and Go forward (ED-2b) in the Editor window: Alt+Left/Right, or
 * Ctrl+- and Ctrl+Shift+- on macOS, and the mouse's back and forward buttons.
 * The keys are taken before the editor sees them, which would otherwise move
 * the caret by a word.
 */
export function useEditorHistoryKeys(enabled: boolean) {
  useEffect(() => {
    if (!enabled) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.isComposing) return;
      const apple = usesMetaModifier();
      const forward = matchesPlatformChord(
        keymapBinding("navigateForward"),
        event,
        apple,
      );
      if (
        (!forward &&
          !matchesPlatformChord(keymapBinding("navigateBack"), event, apple)) ||
        typingElsewhere(event.target)
      ) {
        return;
      }
      event.preventDefault();
      event.stopPropagation();
      travel(forward);
    };
    const onMouseUp = (event: MouseEvent) => {
      if (event.button !== 3 && event.button !== 4) return;
      event.preventDefault();
      travel(event.button === 4);
    };
    window.addEventListener("keydown", onKeyDown, true);
    window.addEventListener("mouseup", onMouseUp);
    return () => {
      window.removeEventListener("keydown", onKeyDown, true);
      window.removeEventListener("mouseup", onMouseUp);
    };
  }, [enabled]);
}

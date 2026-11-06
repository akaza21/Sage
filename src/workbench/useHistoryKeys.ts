import { useEffect } from "react";

import { keymapBinding, matchesPlatformChord } from "../domain/keymapRegistry";
import { usesMetaModifier } from "../ui/shortcutLabel";
import { goBackToPreviousChat, goForwardToNextChat } from "./agentChatHistory";
import type { WorkbenchWindowMode } from "./workbenchWindowMode";

function travel(mode: WorkbenchWindowMode, forward: boolean) {
  if (mode === "agent") {
    void (forward ? goForwardToNextChat() : goBackToPreviousChat());
    return;
  }
  // The Editor history loads with the first File; until then there is
  // nowhere to go.
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
 * Go back and Go forward: Alt+Left/Right, or Ctrl+- and Ctrl+Shift+- on
 * macOS, and the mouse's back and forward buttons. The Editor window travels
 * through places in Files (ED-2b), taking the keys before the editor sees
 * them, which would otherwise move the caret by a word; the Agent window
 * travels through the chats it has shown. `null` (Settings over the window)
 * leaves the keys alone.
 */
export function useHistoryKeys(mode: WorkbenchWindowMode | null) {
  useEffect(() => {
    if (!mode) return;
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
      travel(mode, forward);
    };
    const onMouseUp = (event: MouseEvent) => {
      if (event.button !== 3 && event.button !== 4) return;
      event.preventDefault();
      travel(mode, event.button === 4);
    };
    window.addEventListener("keydown", onKeyDown, true);
    window.addEventListener("mouseup", onMouseUp);
    return () => {
      window.removeEventListener("keydown", onKeyDown, true);
      window.removeEventListener("mouseup", onMouseUp);
    };
  }, [mode]);
}

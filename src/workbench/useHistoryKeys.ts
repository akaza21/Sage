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
  void import("../app/editorHistory").then((history) =>
    forward ? history.goForward() : history.goBack(),
  );
}

function typingElsewhere(target: EventTarget | null) {
  return (
    target instanceof Element &&
    !!target.closest("input, textarea, select, [contenteditable]") &&
    !target.closest(".cm-editor")
  );
}

/**
 * The Editor window takes the keys before the editor sees them, which would
 * otherwise move the caret by a word.
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

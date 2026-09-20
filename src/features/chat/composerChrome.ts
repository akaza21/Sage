import { useEffect, type RefObject } from "react";

export const COMPOSER_PROMPT_PLACEHOLDER =
  "Ask Sage to build, investigate, or explain…";
export const COMPOSER_PROMPT_PLACEHOLDER_DISABLED =
  "Choose an available harness before starting a session";

export function composerShortcutLabel(usesMetaModifier: boolean): string {
  return usesMetaModifier === true ? "⌘L" : "Ctrl+L";
}

export function composerPromptPlaceholder(canPrompt: boolean): string {
  if (canPrompt !== true) {
    return COMPOSER_PROMPT_PLACEHOLDER_DISABLED;
  }
  return COMPOSER_PROMPT_PLACEHOLDER;
}

export function isComposerFocusChord(event: {
  key: string;
  altKey: boolean;
  ctrlKey: boolean;
  metaKey: boolean;
  shiftKey: boolean;
}): boolean {
  if (event.altKey || event.shiftKey) {
    return false;
  }
  if (!(event.ctrlKey || event.metaKey)) {
    return false;
  }
  return event.key.toLowerCase() === "l";
}

export function useComposerFocusHotkey(
  composerRef: RefObject<HTMLTextAreaElement | null>,
  enabled: boolean,
): void {
  useEffect(() => {
    if (enabled !== true) {
      return;
    }
    const onKeyDown = (event: KeyboardEvent) => {
      if (!isComposerFocusChord(event)) {
        return;
      }
      const composer = composerRef.current;
      if (!composer || composer.disabled) {
        return;
      }
      event.preventDefault();
      composer.focus();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [composerRef, enabled]);
}

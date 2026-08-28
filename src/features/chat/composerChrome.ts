import { useEffect, type RefObject } from "react";

export const COMPOSER_WORKSPACE_NAME_CAP = 48;
export const COMPOSER_PROMPT_PLACEHOLDER_DISABLED =
  "Choose an available harness before starting a session";

export function composerShortcutLabel(usesMetaModifier: boolean): string {
  return usesMetaModifier === true ? "⌘L" : "Ctrl+L";
}

export function composerPromptPlaceholder(
  canPrompt: boolean,
  shortcutLabel: string,
): string {
  if (canPrompt !== true) {
    return COMPOSER_PROMPT_PLACEHOLDER_DISABLED;
  }
  if (shortcutLabel === "Ctrl+L" || shortcutLabel === "⌘L") {
    return `Ask anything (${shortcutLabel})`;
  }
  return "Ask anything";
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

export function composerWorkspaceName(
  project: { name: string } | null | undefined,
): string | null {
  if (!project) return null;
  const name = project.name.trim();
  if (
    !name ||
    name.split("").some((character) => {
      const code = character.charCodeAt(0);
      return code < 32 || code === 127;
    })
  ) {
    return null;
  }
  if (name.length <= COMPOSER_WORKSPACE_NAME_CAP) return name;
  return `${name.slice(0, COMPOSER_WORKSPACE_NAME_CAP - 1)}…`;
}

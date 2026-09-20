import { describe, expect, it } from "vitest";

import {
  COMPOSER_PROMPT_PLACEHOLDER,
  composerPromptPlaceholder,
  composerShortcutLabel,
  isComposerFocusChord,
} from "./composerChrome";

describe("composer prompt placeholder", () => {
  it("uses the target prompt independently of the focus shortcut", () => {
    expect(composerPromptPlaceholder(true)).toBe(COMPOSER_PROMPT_PLACEHOLDER);
    expect(composerPromptPlaceholder(false)).toBe(
      "Choose an available harness before starting a session",
    );
  });

  it("labels the Windows chord Ctrl+L and the Mac chord ⌘L", () => {
    expect(composerShortcutLabel(false)).toBe("Ctrl+L");
    expect(composerShortcutLabel(true)).toBe("⌘L");
  });

  it("admits only Ctrl/Cmd+L without alt or shift", () => {
    expect(
      isComposerFocusChord({
        key: "l",
        altKey: false,
        ctrlKey: true,
        metaKey: false,
        shiftKey: false,
      }),
    ).toBe(true);
    expect(
      isComposerFocusChord({
        key: "L",
        altKey: false,
        ctrlKey: false,
        metaKey: true,
        shiftKey: false,
      }),
    ).toBe(true);
    expect(
      isComposerFocusChord({
        key: "l",
        altKey: true,
        ctrlKey: true,
        metaKey: false,
        shiftKey: false,
      }),
    ).toBe(false);
    expect(
      isComposerFocusChord({
        key: "g",
        altKey: false,
        ctrlKey: true,
        metaKey: false,
        shiftKey: false,
      }),
    ).toBe(false);
  });
});

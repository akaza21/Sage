import { describe, expect, it } from "vitest";

import {
  COMPOSER_WORKSPACE_NAME_CAP,
  composerPromptPlaceholder,
  composerShortcutLabel,
  composerWorkspaceName,
  isComposerFocusChord,
} from "./composerChrome";

describe("composer workspace name", () => {
  it("trims a bounded project name and rejects empty or control input", () => {
    expect(composerWorkspaceName({ name: "  micrograd  " })).toBe("micrograd");
    expect(
      composerWorkspaceName({ name: "x".repeat(COMPOSER_WORKSPACE_NAME_CAP) }),
    ).toBe("x".repeat(COMPOSER_WORKSPACE_NAME_CAP));
    expect(
      composerWorkspaceName({
        name: `x${"y".repeat(COMPOSER_WORKSPACE_NAME_CAP)}`,
      }),
    ).toBe(`x${"y".repeat(COMPOSER_WORKSPACE_NAME_CAP - 2)}…`);
    expect(composerWorkspaceName({ name: "   " })).toBeNull();
    expect(composerWorkspaceName({ name: "bad\nname" })).toBeNull();
    expect(composerWorkspaceName(null)).toBeNull();
  });
});

describe("composer prompt placeholder", () => {
  it("shows Ask anything with an admitted shortcut and hides junk shortcuts", () => {
    expect(composerPromptPlaceholder(true, "Ctrl+L")).toBe(
      "Ask anything (Ctrl+L)",
    );
    expect(composerPromptPlaceholder(true, "⌘L")).toBe("Ask anything (⌘L)");
    expect(composerPromptPlaceholder(true, "Alt+L")).toBe("Ask anything");
    expect(composerPromptPlaceholder(false, "Ctrl+L")).toBe(
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

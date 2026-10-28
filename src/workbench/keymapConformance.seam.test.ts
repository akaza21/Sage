import { describe, expect, it, vi } from "vitest";

import {
  KEYMAP_BINDINGS,
  formatKeymapChords,
  keymapBinding,
  matchesKeymapChord,
  matchesPlatformChord,
  type KeymapBinding,
} from "../domain/keymapRegistry";
import { isComposerFocusChord } from "../features/chat/composerChrome";
import {
  handleFileSurfaceCommandKey,
  type FileSurfaceCommandActions,
} from "../features/files/fileSurfaceCommands";

function chordEvent(binding: KeymapBinding) {
  return {
    key:
      binding.key === "escape"
        ? "Escape"
        : binding.key === "f12" || binding.key === "f3"
          ? binding.key.toUpperCase()
          : binding.key,
    altKey: false,
    ctrlKey: binding.ctrlOrMeta === "required",
    metaKey: false,
    shiftKey: binding.shift === "required",
    preventDefault: () => {},
  };
}

function fileActions(
  overrides: Partial<FileSurfaceCommandActions> = {},
): FileSurfaceCommandActions {
  return {
    previewing: false,
    findOpen: false,
    goToOpen: false,
    closePreview: vi.fn(),
    closeFind: vi.fn(),
    closeGoToLine: vi.fn(),
    openFind: vi.fn(),
    openReplace: vi.fn(),
    moveFind: vi.fn(),
    openGoToLine: vi.fn(),
    goToLocalSymbol: vi.fn(),
    save: vi.fn(),
    ...overrides,
  };
}

describe("keymap registry conformance", () => {
  it("keeps every binding id unique", () => {
    const ids = KEYMAP_BINDINGS.map((binding) => binding.id);
    expect(new Set(ids).size).toBe(ids.length);
  });

  it("stores every binding key already lowercased", () => {
    for (const binding of KEYMAP_BINDINGS)
      expect(binding.key).toBe(binding.key.toLowerCase());
  });

  const fileSurface: Array<
    [
      string,
      keyof FileSurfaceCommandActions,
      Partial<FileSurfaceCommandActions>,
    ]
  > = [
    ["save", "save", {}],
    ["find", "openFind", {}],
    ["replace", "openReplace", {}],
    ["findNext", "moveFind", {}],
    ["findPrevious", "moveFind", {}],
    ["goToLine", "openGoToLine", {}],
    ["goToSymbol", "goToLocalSymbol", {}],
    ["dismiss", "closeFind", { findOpen: true }],
  ];

  for (const [id, action, overrides] of fileSurface) {
    it(`drives the real file surface handler from the ${id} binding`, () => {
      const actions = fileActions(overrides);
      const handled = handleFileSurfaceCommandKey(
        chordEvent(keymapBinding(id)),
        actions,
      );
      expect(handled).toBe(true);
      expect(actions[action]).toHaveBeenCalled();
    });
  }

  it("agrees with the composer focus handler on its own chord", () => {
    const binding = keymapBinding("focusComposer");
    expect(isComposerFocusChord(chordEvent(binding))).toBe(true);
    expect(matchesKeymapChord(binding, chordEvent(binding))).toBe(true);
  });

  it("agrees with the composer focus handler on a near miss", () => {
    const binding = keymapBinding("focusComposer");
    const shifted = { ...chordEvent(binding), shiftKey: true };
    expect(isComposerFocusChord(shifted)).toBe(false);
    expect(matchesKeymapChord(binding, shifted)).toBe(false);
  });

  it("rejects an unmodified key for a chord that requires the modifier", () => {
    const binding = keymapBinding("save");
    expect(
      matchesKeymapChord(binding, { ...chordEvent(binding), ctrlKey: false }),
    ).toBe(false);
  });

  it("rejects a modified key for a chord that forbids the modifier", () => {
    const binding = keymapBinding("goToSymbol");
    expect(
      matchesKeymapChord(binding, { ...chordEvent(binding), ctrlKey: true }),
    ).toBe(false);
  });

  it("honours the modifiers each binding declares", () => {
    expect(
      matchesKeymapChord(keymapBinding("save"), {
        ...chordEvent(keymapBinding("save")),
        shiftKey: true,
      }),
    ).toBe(true);
    expect(
      matchesKeymapChord(keymapBinding("find"), {
        ...chordEvent(keymapBinding("find")),
        shiftKey: true,
      }),
    ).toBe(false);
  });

  it("renders platform-appropriate chord labels", () => {
    expect(formatKeymapChords(keymapBinding("save"), false)).toEqual([
      "Ctrl+S",
    ]);
    expect(formatKeymapChords(keymapBinding("save"), true)).toEqual(["Cmd+S"]);
    expect(formatKeymapChords(keymapBinding("openSettings"), true)).toEqual([
      "Cmd+,",
    ]);
    expect(formatKeymapChords(keymapBinding("goToSymbol"), false)).toEqual([
      "F12",
    ]);
    expect(formatKeymapChords(keymapBinding("dismiss"), false)).toEqual([
      "Esc",
    ]);
  });

  it("labels Replace and Find next the way VS Code does on each platform", () => {
    const labels = (id: string) =>
      [false, true].map((apple) =>
        formatKeymapChords(keymapBinding(id), apple).join(" "),
      );
    expect(labels("replace")).toEqual(["Ctrl+H", "Cmd+Option+F"]);
    expect(labels("findNext")).toEqual(["F3", "Cmd+G"]);
    expect(labels("findPrevious")).toEqual(["Shift+F3", "Cmd+Shift+G"]);
  });

  it("labels Go to Line with Control on every platform", () => {
    expect(formatKeymapChords(keymapBinding("goToLine"), false)).toEqual([
      "Ctrl+G",
    ]);
    expect(formatKeymapChords(keymapBinding("goToLine"), true)).toEqual([
      "Ctrl+G",
    ]);
  });

  it("matches only the platform's own modifier for Editor chords", () => {
    const save = keymapBinding("save");
    const base = { ...chordEvent(save), ctrlKey: false };
    expect(matchesPlatformChord(save, { ...base, ctrlKey: true }, false)).toBe(
      true,
    );
    expect(matchesPlatformChord(save, { ...base, metaKey: true }, false)).toBe(
      false,
    );
    expect(matchesPlatformChord(save, { ...base, metaKey: true }, true)).toBe(
      true,
    );
    expect(matchesPlatformChord(save, { ...base, ctrlKey: true }, true)).toBe(
      false,
    );
    const goToLine = keymapBinding("goToLine");
    const g = { ...chordEvent(goToLine), ctrlKey: false };
    expect(matchesPlatformChord(goToLine, { ...g, ctrlKey: true }, true)).toBe(
      true,
    );
    expect(matchesPlatformChord(goToLine, { ...g, metaKey: true }, true)).toBe(
      false,
    );
    const commit = keymapBinding("commit");
    const enter = { ...chordEvent(commit), ctrlKey: false };
    expect(
      matchesPlatformChord(commit, { ...enter, metaKey: true }, true),
    ).toBe(true);
    expect(matchesPlatformChord(commit, enter, true)).toBe(false);
  });

  it("labels the composer and commit chords the way the UI states them", () => {
    expect(formatKeymapChords(keymapBinding("sendMessage"), false)).toEqual([
      "Enter",
      "Ctrl+Enter",
    ]);
    expect(formatKeymapChords(keymapBinding("sendMessage"), true)).toEqual([
      "Enter",
      "Cmd+Enter",
    ]);
    expect(formatKeymapChords(keymapBinding("newline"), false)).toEqual([
      "Shift+Enter",
    ]);
    expect(formatKeymapChords(keymapBinding("commit"), false)).toEqual([
      "Ctrl+Enter",
    ]);
    expect(formatKeymapChords(keymapBinding("commit"), true)).toEqual([
      "Cmd+Enter",
    ]);
  });

  it("sends on plain Enter and Ctrl+Enter but never on Shift+Enter", () => {
    const plain = {
      key: "Enter",
      altKey: false,
      ctrlKey: false,
      metaKey: false,
      shiftKey: false,
    };
    const shifted = { ...plain, shiftKey: true };
    const controlled = { ...plain, ctrlKey: true };
    const metaPressed = { ...plain, metaKey: true };
    expect(matchesKeymapChord(keymapBinding("sendMessage"), plain)).toBe(true);
    expect(matchesKeymapChord(keymapBinding("sendMessage"), shifted)).toBe(
      false,
    );
    expect(matchesKeymapChord(keymapBinding("sendMessage"), controlled)).toBe(
      true,
    );
    expect(matchesKeymapChord(keymapBinding("sendMessage"), metaPressed)).toBe(
      true,
    );
    expect(
      matchesKeymapChord(keymapBinding("sendMessage"), {
        ...controlled,
        shiftKey: true,
      }),
    ).toBe(false);
    expect(matchesKeymapChord(keymapBinding("newline"), shifted)).toBe(true);
    expect(matchesKeymapChord(keymapBinding("newline"), plain)).toBe(false);
    expect(matchesKeymapChord(keymapBinding("commit"), controlled)).toBe(true);
    expect(matchesKeymapChord(keymapBinding("commit"), plain)).toBe(false);
  });
});

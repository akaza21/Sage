import { expect, it } from "vitest";
import {
  formatKeymapChords,
  KEYMAP_BINDINGS,
  keymapBinding,
  matchesKeymapChord,
  matchesPlatformChord,
} from "./keymapRegistry";
import { KEYMAP_DESCRIPTIONS } from "./keymapDescriptions";
import { EDITOR_EDITING_BINDINGS } from "./settingsNavigation";

it.each([
  ["agent.changes", "g", true, false],
  ["agent.terminal", "`", false, false],
  ["agent.files", "p", false, false],
  ["agent.sideChat", "s", false, true],
])(
  "binds %s without an overlapping Agent or global chord",
  (id, key, shiftKey, altKey) => {
    const event = { key, shiftKey, altKey, ctrlKey: true, metaKey: false };
    expect(
      KEYMAP_BINDINGS.filter(
        (binding) =>
          binding.scope !== "Editor" && matchesKeymapChord(binding, event),
      ).map((binding) => binding.id),
    ).toEqual([id]);
  },
);

it("keeps chat and editor find as distinct scoped bindings", () => {
  const event = {
    key: "f",
    ctrlKey: true,
    metaKey: false,
    shiftKey: false,
    altKey: false,
  };
  expect(
    KEYMAP_BINDINGS.filter(
      (binding) =>
        binding.scope === "Agent chat" && matchesKeymapChord(binding, event),
    ).map((binding) => binding.id),
  ).toEqual(["chat.findInChat"]);
  expect(
    KEYMAP_BINDINGS.filter(
      (binding) =>
        binding.scope === "Editor" && matchesKeymapChord(binding, event),
    ).map((binding) => binding.id),
  ).toEqual(["find"]);
  expect(KEYMAP_DESCRIPTIONS["chat.findInChat"]).toBe(
    "Search the visible chat's messages.",
  );
});

it("gives Quick Open a chord no other Editor or Global binding uses", () => {
  const quickOpen = keymapBinding("quickOpen");
  expect(quickOpen.reservedFor).toBeUndefined();
  const event = {
    key: "p",
    ctrlKey: true,
    metaKey: false,
    shiftKey: false,
    altKey: false,
  };
  const matches = KEYMAP_BINDINGS.filter((binding) =>
    matchesKeymapChord(binding, event),
  );
  expect(
    matches
      .filter((binding) => binding.scope !== "Agent")
      .map((binding) => binding.id),
  ).toEqual(["quickOpen"]);
  // CP-48: in Agent mode only, Ctrl+P opens this Chat's Files picker (Codex 67).
  expect(
    matches
      .filter((binding) => binding.scope === "Agent")
      .map((binding) => binding.id),
  ).toEqual(["agent.files"]);
});

it("names Option on macOS and follows a binding's macOS modifiers", () => {
  const fold = EDITOR_EDITING_BINDINGS.find(
    (binding) => binding.id === "editor.fold",
  )!;
  expect(formatKeymapChords(fold, false)).toEqual(["Ctrl+Shift+["]);
  expect(formatKeymapChords(fold, true)).toEqual(["Cmd+Option+["]);
  const event = {
    key: "[",
    ctrlKey: false,
    metaKey: true,
    shiftKey: false,
    altKey: true,
  };
  expect(matchesPlatformChord(fold, event, true)).toBe(true);
  expect(matchesPlatformChord(fold, { ...event, shiftKey: true }, true)).toBe(
    false,
  );
  const cursorAbove = EDITOR_EDITING_BINDINGS.find(
    (binding) => binding.id === "editor.addCursorAbove",
  )!;
  expect(formatKeymapChords(cursorAbove, false)).toEqual(["Ctrl+Alt+Up"]);
  expect(formatKeymapChords(cursorAbove, true)).toEqual(["Cmd+Option+Up"]);
});

it("leaves Save All's Control+Alt+S to AltGr when AltGr is down", () => {
  const saveAll = keymapBinding("saveAll");
  const event = {
    key: "s",
    ctrlKey: true,
    metaKey: false,
    shiftKey: false,
    altKey: true,
  };
  expect(matchesPlatformChord(saveAll, event, false)).toBe(true);
  const altGraph = {
    ...event,
    getModifierState: (key: string) => key === "AltGraph",
  };
  expect(matchesPlatformChord(saveAll, altGraph, false)).toBe(false);
});

import { expect, it } from "vitest";
import {
  formatKeymapChords,
  KEYMAP_BINDINGS,
  keymapBinding,
  matchesKeymapChord,
  matchesPlatformChord,
} from "./keymapRegistry";
import { EDITOR_EDITING_BINDINGS } from "./settingsNavigation";

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
  expect(keymapBinding("chat.findInChat").description).toBe(
    "Search the visible chat's messages.",
  );
});

it("reserves Quick Open for EP-08 on a chord no live binding uses", () => {
  const quickOpen = keymapBinding("quickOpen");
  expect(quickOpen.reservedFor).toBe("EP-08");
  const event = {
    key: "p",
    ctrlKey: true,
    metaKey: false,
    shiftKey: false,
    altKey: false,
  };
  expect(
    KEYMAP_BINDINGS.filter((binding) => matchesKeymapChord(binding, event)).map(
      (binding) => binding.id,
    ),
  ).toEqual(["quickOpen"]);
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

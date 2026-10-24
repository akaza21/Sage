import { expect, it } from "vitest";
import {
  KEYMAP_BINDINGS,
  keymapBinding,
  matchesKeymapChord,
} from "./keymapRegistry";

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

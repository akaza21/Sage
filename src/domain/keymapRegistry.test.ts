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

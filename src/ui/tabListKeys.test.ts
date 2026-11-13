import { describe, expect, it } from "vitest";

import { tabIndexForKey } from "./tabListKeys";

const none = [false, false, false, false];

describe("tabIndexForKey", () => {
  it.each([
    ["ArrowRight", 1, 2],
    ["ArrowRight", 3, 0],
    ["ArrowLeft", 0, 3],
    ["ArrowLeft", 2, 1],
    ["Home", 2, 0],
    ["End", 1, 3],
  ])("horizontal %s from %i goes to %i", (key, from, to) => {
    expect(tabIndexForKey(key, from, none, "horizontal")).toBe(to);
  });

  it("uses only the arrows of its orientation", () => {
    expect(tabIndexForKey("ArrowDown", 0, none, "horizontal")).toBeNull();
    expect(tabIndexForKey("ArrowRight", 0, none, "vertical")).toBeNull();
    expect(tabIndexForKey("ArrowDown", 3, none, "vertical")).toBe(0);
    expect(tabIndexForKey("ArrowUp", 1, none, "vertical")).toBe(0);
  });

  it("passes over disabled tabs, including at the ends", () => {
    const disabled = [true, false, true, false];
    expect(tabIndexForKey("ArrowRight", 1, disabled, "horizontal")).toBe(3);
    expect(tabIndexForKey("ArrowRight", 3, disabled, "horizontal")).toBe(1);
    expect(tabIndexForKey("Home", 3, disabled, "horizontal")).toBe(1);
    expect(
      tabIndexForKey("End", 1, [false, true, false, true], "horizontal"),
    ).toBe(2);
  });

  it("ignores other keys and lists with nowhere to go", () => {
    expect(tabIndexForKey("Enter", 0, none, "horizontal")).toBeNull();
    expect(tabIndexForKey("ArrowRight", 0, [false], "horizontal")).toBeNull();
    expect(tabIndexForKey("Home", 0, [true, true], "horizontal")).toBeNull();
  });
});

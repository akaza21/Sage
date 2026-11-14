import { describe, expect, it } from "vitest";

import {
  hasNavigationModifier,
  listIndexForKey,
  typeaheadIndex,
} from "./listNavigation";

const none = [false, false, false, false];
const list = { orientation: "vertical", wrap: true } as const;
const tree = { orientation: "vertical", wrap: false } as const;

describe("listIndexForKey", () => {
  it.each([
    ["ArrowDown", 0, 1],
    ["ArrowDown", 3, 0],
    ["ArrowUp", 0, 3],
    ["Home", 2, 0],
    ["End", 1, 3],
  ])("wraps %s from %i to %i", (key, from, to) => {
    expect(listIndexForKey(key, from, none, list)).toBe(to);
  });

  it("starts from the matching end when nothing is active yet", () => {
    expect(listIndexForKey("ArrowDown", -1, none, list)).toBe(0);
    expect(listIndexForKey("ArrowUp", -1, none, list)).toBe(3);
    const ends = [true, false, false, true];
    expect(listIndexForKey("ArrowDown", -1, ends, list)).toBe(1);
    expect(listIndexForKey("ArrowUp", -1, ends, list)).toBe(2);
  });

  it("stops at the ends when the list does not wrap", () => {
    expect(listIndexForKey("ArrowDown", 3, none, tree)).toBeNull();
    expect(listIndexForKey("ArrowUp", 0, none, tree)).toBeNull();
    expect(listIndexForKey("ArrowDown", 1, none, tree)).toBe(2);
    expect(
      listIndexForKey("ArrowDown", 1, [false, false, true, true], tree),
    ).toBeNull();
  });

  it("passes over disabled items", () => {
    const disabled = [true, false, true, false];
    expect(listIndexForKey("ArrowDown", 1, disabled, list)).toBe(3);
    expect(listIndexForKey("ArrowDown", 3, disabled, list)).toBe(1);
    expect(listIndexForKey("Home", 3, disabled, list)).toBe(1);
    expect(listIndexForKey("End", 1, [false, true, false, true], list)).toBe(2);
  });

  it("uses the arrows of its orientation only", () => {
    const row = { orientation: "horizontal", wrap: true } as const;
    expect(listIndexForKey("ArrowRight", 0, none, row)).toBe(1);
    expect(listIndexForKey("ArrowDown", 0, none, row)).toBeNull();
    expect(listIndexForKey("ArrowRight", 0, none, list)).toBeNull();
  });

  it("pages by the page size and stops at the ends", () => {
    const ten = Array<boolean>(10).fill(false);
    const paged = { ...list, pageSize: 3 };
    expect(listIndexForKey("PageDown", 2, ten, paged)).toBe(5);
    expect(listIndexForKey("PageDown", 8, ten, paged)).toBe(9);
    expect(listIndexForKey("PageUp", 1, ten, paged)).toBe(0);
    expect(listIndexForKey("PageDown", -1, ten, paged)).toBe(2);
    expect(listIndexForKey("PageUp", -1, ten, paged)).toBe(0);
    expect(listIndexForKey("PageDown", 9, ten, paged)).toBeNull();
    expect(listIndexForKey("PageDown", 2, ten, list)).toBeNull();
  });

  it("pages onto the nearest enabled item", () => {
    const paged = { ...list, pageSize: 2 };
    const disabled = [false, false, true, false, true];
    expect(listIndexForKey("PageDown", 0, disabled, paged)).toBe(3);
    expect(listIndexForKey("PageDown", 3, disabled, paged)).toBeNull();
  });

  it("returns null for other keys and when no other item can take focus", () => {
    expect(listIndexForKey("Enter", 0, none, list)).toBeNull();
    expect(listIndexForKey("ArrowDown", 0, [false], list)).toBeNull();
    expect(listIndexForKey("ArrowDown", -1, [true, true], list)).toBeNull();
    expect(listIndexForKey("Home", 0, [true, true], list)).toBeNull();
    expect(listIndexForKey("ArrowDown", -1, [], list)).toBeNull();
  });
});

describe("typeaheadIndex", () => {
  const labels = ["Copy", "Cut", " Paste", "Close", "Delete"];

  it("moves to the next item that starts with a repeated letter", () => {
    expect(typeaheadIndex(labels, -1, "c")).toBe(0);
    expect(typeaheadIndex(labels, 0, "c")).toBe(1);
    expect(typeaheadIndex(labels, 1, "cc")).toBe(3);
    expect(typeaheadIndex(labels, 3, "ccc")).toBe(0);
  });

  it("keeps the active item while a longer query still matches it", () => {
    expect(typeaheadIndex(labels, 1, "cu")).toBe(1);
    expect(typeaheadIndex(labels, 1, "cl")).toBe(3);
    expect(typeaheadIndex(labels, 0, "pa")).toBe(2);
  });

  it("ignores case and leading space", () => {
    expect(typeaheadIndex(labels, -1, "P")).toBe(2);
    expect(typeaheadIndex(labels, -1, "pA")).toBe(2);
  });

  it("returns -1 without a match", () => {
    expect(typeaheadIndex(labels, 0, "x")).toBe(-1);
    expect(typeaheadIndex(labels, 0, "")).toBe(-1);
  });
});

describe("hasNavigationModifier", () => {
  it("is true for any modifier", () => {
    const keys = {
      altKey: false,
      ctrlKey: false,
      metaKey: false,
      shiftKey: false,
    };
    expect(hasNavigationModifier(keys)).toBe(false);
    for (const modifier of Object.keys(keys))
      expect(hasNavigationModifier({ ...keys, [modifier]: true })).toBe(true);
  });
});

import { act, renderHook } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { ACTIVE_OPTION_PAGE_SIZE, useActiveOption } from "./listNavigation";

const ids = (count: number) =>
  Array.from({ length: count }, (_, index) => `row-${index}`);

describe("useActiveOption", () => {
  it("starts on the first row or on none, as asked", () => {
    expect(
      renderHook(() => useActiveOption(ids(3), "first")).result.current,
    ).toMatchObject({ activeId: "row-0", activeIndex: 0 });
    expect(
      renderHook(() => useActiveOption(ids(3), "none")).result.current,
    ).toMatchObject({ activeId: null, activeIndex: -1 });
    expect(
      renderHook(() => useActiveOption([], "first")).result.current,
    ).toMatchObject({ activeId: null, activeIndex: -1 });
  });

  it("moves with the arrows and wraps past the ends", () => {
    const { result } = renderHook(() => useActiveOption(ids(3), "none"));
    act(() => result.current.move("ArrowDown"));
    expect(result.current.activeId).toBe("row-0");
    act(() => result.current.move("ArrowUp"));
    expect(result.current.activeId).toBe("row-2");
    act(() => result.current.move("ArrowDown"));
    expect(result.current.activeId).toBe("row-0");
    const fresh = renderHook(() => useActiveOption(ids(3), "none")).result;
    act(() => fresh.current.move("ArrowUp"));
    expect(fresh.current.activeId).toBe("row-2");
  });

  it("pages by a fixed size and stops at the ends", () => {
    const count = ACTIVE_OPTION_PAGE_SIZE + 5;
    const { result } = renderHook(() => useActiveOption(ids(count), "first"));
    act(() => result.current.move("PageDown"));
    expect(result.current.activeIndex).toBe(ACTIVE_OPTION_PAGE_SIZE);
    act(() => result.current.move("PageDown"));
    expect(result.current.activeIndex).toBe(count - 1);
    act(() => result.current.move("PageUp"));
    expect(result.current.activeIndex).toBe(
      count - 1 - ACTIVE_OPTION_PAGE_SIZE,
    );
    act(() => result.current.move("Home"));
    expect(result.current.activeIndex).toBe(0);
    act(() => result.current.move("End"));
    expect(result.current.activeIndex).toBe(count - 1);
  });

  it("ignores keys that do not navigate", () => {
    const { result } = renderHook(() => useActiveOption(ids(3), "first"));
    act(() => result.current.move("ArrowRight"));
    act(() => result.current.move("a"));
    expect(result.current.activeIndex).toBe(0);
  });

  it("follows its row when the list is reordered, and falls back when it goes", () => {
    const { result, rerender } = renderHook(
      ({ list, fallback }) => useActiveOption(list, fallback),
      {
        initialProps: {
          list: ["a", "b", "c"],
          fallback: "first" as "first" | "none",
        },
      },
    );
    act(() => result.current.setActiveId("b"));
    rerender({ list: ["c", "b", "a"], fallback: "first" });
    expect(result.current).toMatchObject({ activeId: "b", activeIndex: 1 });
    rerender({ list: ["c", "a"], fallback: "first" });
    expect(result.current).toMatchObject({ activeId: "c", activeIndex: 0 });
    expect(result.current.chosenId).toBe("b");
    rerender({ list: ["c", "a"], fallback: "none" });
    expect(result.current).toMatchObject({ activeId: null, activeIndex: -1 });
    rerender({ list: ["a", "b"], fallback: "none" });
    expect(result.current).toMatchObject({ activeId: "b", activeIndex: 1 });
  });
});

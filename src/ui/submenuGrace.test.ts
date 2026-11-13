import { renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  graceArea,
  pointInPolygon,
  SUBMENU_GRACE_MS,
  useSubmenuGrace,
} from "./submenuGrace";

const submenu = DOMRect.fromRect({ x: 200, y: 0, width: 160, height: 80 });

afterEach(() => vi.useRealTimers());

describe("graceArea", () => {
  it("covers the way from the exit point to a submenu on the right", () => {
    const area = graceArea({ x: 150, y: 20 }, submenu)!;
    expect(pointInPolygon({ x: 180, y: 25 }, area)).toBe(true);
    expect(pointInPolygon({ x: 199, y: 70 }, area)).toBe(true);
    expect(pointInPolygon({ x: 180, y: 75 }, area)).toBe(false);
    expect(pointInPolygon({ x: 140, y: 20 }, area)).toBe(false);
  });

  it("covers a submenu placed to the left", () => {
    const left = DOMRect.fromRect({ x: 0, y: 0, width: 100, height: 80 });
    const area = graceArea({ x: 150, y: 20 }, left)!;
    expect(pointInPolygon({ x: 120, y: 25 }, area)).toBe(true);
    expect(pointInPolygon({ x: 160, y: 20 }, area)).toBe(false);
  });
});

describe("graceArea for a stacked submenu", () => {
  it("has none when a narrow window places the submenu above or below", () => {
    const below = DOMRect.fromRect({ x: 100, y: 120, width: 160, height: 80 });
    expect(graceArea({ x: 150, y: 20 }, below)).toBeNull();
  });
});

describe("useSubmenuGrace", () => {
  const element = { getBoundingClientRect: () => submenu } as Element;

  it("holds moves inside the area, then lets go once one leaves it", () => {
    const { result } = renderHook(() => useSubmenuGrace());
    result.current.leave({ clientX: 150, clientY: 20 }, element);
    expect(result.current.holds({ clientX: 180, clientY: 25 })).toBe(true);
    expect(result.current.holds({ clientX: 100, clientY: 40 })).toBe(false);
    expect(result.current.holds({ clientX: 180, clientY: 25 })).toBe(false);
  });

  it("lets go after the grace time", () => {
    vi.useFakeTimers();
    const { result } = renderHook(() => useSubmenuGrace());
    result.current.leave({ clientX: 150, clientY: 20 }, element);
    vi.advanceTimersByTime(SUBMENU_GRACE_MS);
    expect(result.current.holds({ clientX: 180, clientY: 25 })).toBe(false);
  });

  it("lets go when the open submenu closes", () => {
    const { result } = renderHook(() => useSubmenuGrace());
    result.current.leave({ clientX: 150, clientY: 20 }, element);
    result.current.reset();
    expect(result.current.holds({ clientX: 180, clientY: 25 })).toBe(false);
  });

  it("holds nothing without a submenu", () => {
    const { result } = renderHook(() => useSubmenuGrace());
    result.current.leave({ clientX: 150, clientY: 20 }, null);
    expect(result.current.holds({ clientX: 180, clientY: 25 })).toBe(false);
  });
});

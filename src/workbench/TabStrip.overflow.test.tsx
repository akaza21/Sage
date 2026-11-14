import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../app/store";
import type { WorkbenchTab } from "../domain/workbench";
import { TabStrip } from "./TabStrip";

const initialState = useWorkbenchStore.getState();

afterEach(() => {
  cleanup();
  useWorkbenchStore.setState(initialState, true);
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

const fileTab = (index: number): WorkbenchTab => ({
  id: `file:${index}`,
  kind: "file",
  title: `file${index}.ts`,
  position: index,
  pinned: false,
  dirty: false,
  resource: { rootId: "root-a", relativePath: `src/deep/file${index}.ts` },
});

function installTabs(count: number, activeIndex: number) {
  useWorkbenchStore.setState({
    tabs: Array.from({ length: count }, (_, index) => fileTab(index)),
    activeTabId: `file:${activeIndex}`,
    activateTab: vi.fn(async () => undefined),
  });
}

// jsdom has no layout: tabs are 120 px wide in a 360 px strip.
function layOut(strip: HTMLElement) {
  Object.defineProperty(strip, "clientWidth", {
    value: 360,
    configurable: true,
  });
  Object.defineProperty(strip, "scrollWidth", { value: 120 * 12 });
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
    function (this: HTMLElement) {
      if (this === strip) return DOMRect.fromRect({ x: 0, width: 360 });
      const index = [...strip.children].indexOf(this);
      return DOMRect.fromRect({
        x: index * 120 - strip.scrollLeft,
        width: 120,
      });
    },
  );
}

describe("TabStrip overflow", () => {
  it("names each tab's full path in its tooltip", () => {
    installTabs(2, 0);
    render(<TabStrip />);
    expect(screen.getByRole("tab", { name: "file1.ts" })).toHaveAttribute(
      "title",
      "src/deep/file1.ts",
    );
  });

  it("scrolls a newly active tab into view", () => {
    installTabs(12, 0);
    const { container, rerender } = render(<TabStrip />);
    const strip = container.querySelector<HTMLElement>(".tab-strip__scroll")!;
    layOut(strip);
    useWorkbenchStore.setState({ activeTabId: "file:11" });
    rerender(<TabStrip />);
    expect(strip.scrollLeft).toBe(120 * 12 - 360);
    useWorkbenchStore.setState({ activeTabId: "file:1" });
    rerender(<TabStrip />);
    expect(strip.scrollLeft).toBe(120);
  });

  it("keeps manual scrolling when a background tab resizes", () => {
    let notify: ResizeObserverCallback = () => undefined;
    vi.stubGlobal(
      "ResizeObserver",
      class {
        constructor(callback: ResizeObserverCallback) {
          notify = callback;
        }
        observe() {}
        unobserve() {}
        disconnect() {}
      },
    );
    installTabs(12, 0);
    const { container } = render(<TabStrip />);
    const strip = container.querySelector<HTMLElement>(".tab-strip__scroll")!;
    layOut(strip);
    fireEvent.wheel(strip, { deltaY: 600 });
    const resized = (target: Element) =>
      notify([{ target } as ResizeObserverEntry], {} as ResizeObserver);
    resized(strip.children[8]);
    expect(strip.scrollLeft).toBe(600);
    resized(strip.children[0]);
    expect(strip.scrollLeft).toBe(0);
  });

  it("scrolls the strip sideways with a vertical wheel", () => {
    installTabs(12, 0);
    const { container } = render(<TabStrip />);
    const strip = container.querySelector<HTMLElement>(".tab-strip__scroll")!;
    layOut(strip);
    fireEvent.wheel(strip, { deltaY: 100 });
    expect(strip.scrollLeft).toBe(100);
    fireEvent.wheel(strip, { deltaX: 40, deltaY: 10 });
    expect(strip.scrollLeft).toBe(100);
    // A trackpad pinch arrives as a wheel with Ctrl held.
    fireEvent.wheel(strip, { deltaY: 50, ctrlKey: true });
    expect(strip.scrollLeft).toBe(100);
  });

  it("marks the edges that hide tabs so the strip can fade them", () => {
    installTabs(12, 0);
    const { container } = render(<TabStrip />);
    const strip = container.querySelector<HTMLElement>(".tab-strip__scroll")!;
    layOut(strip);
    fireEvent.scroll(strip);
    expect(strip.dataset.overflow).toBe("end");
    strip.scrollLeft = 600;
    fireEvent.scroll(strip);
    expect(strip.dataset.overflow).toBe("start end");
    strip.scrollLeft = 120 * 12 - 360;
    fireEvent.scroll(strip);
    expect(strip.dataset.overflow).toBe("start");
  });

  it("clears the edge marks once a widened strip fits every tab", () => {
    let resized: ResizeObserverCallback | undefined;
    vi.stubGlobal(
      "ResizeObserver",
      class {
        constructor(callback: ResizeObserverCallback) {
          resized = callback;
        }
        observe() {}
        unobserve() {}
        disconnect() {}
      },
    );
    installTabs(12, 0);
    const { container } = render(<TabStrip />);
    const strip = container.querySelector<HTMLElement>(".tab-strip__scroll")!;
    layOut(strip);
    fireEvent.scroll(strip);
    expect(strip.dataset.overflow).toBe("end");
    Object.defineProperty(strip, "clientWidth", { value: 120 * 12 });
    resized!(
      [{ target: strip } as unknown as ResizeObserverEntry],
      {} as ResizeObserver,
    );
    expect(strip).not.toHaveAttribute("data-overflow");
  });
});

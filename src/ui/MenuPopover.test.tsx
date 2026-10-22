import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { Profiler, useId, useRef, useState } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { MenuPopover } from "./MenuPopover";

const INITIAL_INNER_HEIGHT = window.innerHeight;
const INITIAL_INNER_WIDTH = window.innerWidth;

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  Object.defineProperty(window, "innerHeight", {
    configurable: true,
    value: INITIAL_INNER_HEIGHT,
  });
  Object.defineProperty(window, "innerWidth", {
    configurable: true,
    value: INITIAL_INNER_WIDTH,
  });
});

function MenuHarness({
  initialFocus = "first",
  trapFocus = true,
}: {
  initialFocus?: "first" | "last";
  trapFocus?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const menuId = useId();

  return (
    <>
      <button
        ref={triggerRef}
        type="button"
        aria-controls={menuId}
        aria-expanded={open}
        aria-haspopup="menu"
        onClick={() => setOpen((current) => !current)}
      >
        Actions
      </button>
      <MenuPopover
        id={menuId}
        open={open}
        ariaLabel="Actions"
        initialFocus={initialFocus}
        triggerRef={triggerRef}
        trapFocus={trapFocus}
        onDismiss={() => setOpen(false)}
      >
        <button className="menu-item" type="button" role="menuitem">
          First
        </button>
        <button className="menu-item" type="button" role="menuitem" disabled>
          Disabled
        </button>
        <button className="menu-item" type="button" role="menuitem">
          Last
        </button>
      </MenuPopover>
      <button type="button">Outside</button>
    </>
  );
}

function GrowingMenuHarness() {
  const [open, setOpen] = useState(false);
  const [expanded, setExpanded] = useState(false);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const menuId = useId();

  return (
    <>
      <button
        ref={triggerRef}
        type="button"
        aria-controls={menuId}
        aria-expanded={open}
        aria-haspopup="menu"
        onClick={() => setOpen(true)}
      >
        Grow actions
      </button>
      <MenuPopover
        id={menuId}
        open={open}
        ariaLabel="Grow actions"
        focusKey={expanded}
        triggerRef={triggerRef}
        onDismiss={() => setOpen(false)}
      >
        {expanded ? (
          Array.from({ length: 12 }, (_, index) => (
            <button
              key={index}
              className="menu-item"
              type="button"
              role="menuitem"
            >
              Destination {index + 1}
            </button>
          ))
        ) : (
          <button
            className="menu-item"
            type="button"
            role="menuitem"
            onClick={() => setExpanded(true)}
          >
            Expand
          </button>
        )}
      </MenuPopover>
    </>
  );
}

describe("MenuPopover", () => {
  it.each(
    [720, 480].flatMap((viewport) =>
      ["agent-chat-pane", "agent-chat-center"].map(
        (ownerClass) => [viewport, ownerClass] as const,
      ),
    ),
  )("shrinks an oversized menu at %ipx into %s", (viewport, ownerClass) => {
    Object.defineProperty(window, "innerWidth", {
      configurable: true,
      value: viewport,
    });
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
      function (this: HTMLElement) {
        if (this.classList.contains(ownerClass))
          return new DOMRect(300, 40, viewport - 300, 700);
        if (this.classList.contains("composer"))
          return new DOMRect(316, 600, viewport - 332, 120);
        if (this.getAttribute("role") === "menu")
          return new DOMRect(0, 0, 420, 140);
        return new DOMRect(324, 650, 28, 28);
      },
    );
    render(
      <section className={ownerClass}>
        {ownerClass === "agent-chat-pane" ? (
          <form className="composer">
            <MenuHarness />
          </form>
        ) : (
          <header>
            <MenuHarness />
          </header>
        )}
      </section>,
    );
    fireEvent.click(screen.getByRole("button", { name: "Actions" }));
    expect(screen.getByRole("menu", { name: "Actions" })).toHaveStyle({
      left: "300px",
      maxWidth: `${viewport - 308}px`,
    });
  });

  it("keeps a left-edge composer menu inside its composer when it fits", () => {
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
      function (this: HTMLElement) {
        if (this.classList.contains("composer"))
          return new DOMRect(480, 400, 580, 120);
        if (this.getAttribute("role") === "menu")
          return new DOMRect(0, 0, 298, 140);
        return new DOMRect(488, 490, 22, 28);
      },
    );
    render(
      <form className="composer">
        <MenuHarness />
      </form>,
    );
    fireEvent.click(screen.getByRole("button", { name: "Actions" }));
    expect(screen.getByRole("menu", { name: "Actions" })).toHaveStyle({
      left: "480px",
    });
  });

  it.each([
    [140, 360],
    [400, 48],
  ])(
    "clamps a %ipx menu to the chat pane when it fits, otherwise the viewport",
    (height, top) => {
      Object.defineProperty(window, "innerHeight", {
        configurable: true,
        value: 768,
      });
      vi.spyOn(
        HTMLElement.prototype,
        "getBoundingClientRect",
      ).mockImplementation(function (this: HTMLElement) {
        if (this.classList.contains("agent-chat-pane"))
          return new DOMRect(0, 200, 480, 300);
        if (this.getAttribute("role") === "menu")
          return new DOMRect(0, 0, 160, height);
        return new DOMRect(100, 452, 100, 28);
      });
      render(
        <section className="agent-chat-pane">
          <MenuHarness />
        </section>,
      );
      fireEvent.click(screen.getByRole("button", { name: "Actions" }));
      expect(screen.getByRole("menu", { name: "Actions" })).toHaveStyle({
        top: top + "px",
      });
    },
  );

  it.each([
    [100, 244, 308],
    [200, 344, 208],
    [260, 8, 248],
    [388, 64, 320],
    [408, 84, 320],
    [8, 8, 320],
  ])(
    "keeps a nested picker usable at parent y=%i with top=%i and height cap=%i",
    (parentTop, expectedTop, maxHeight) => {
      Object.defineProperty(window, "innerHeight", {
        configurable: true,
        value: 560,
      });
      Object.defineProperty(window, "innerWidth", {
        configurable: true,
        value: 480,
      });
      vi.spyOn(
        HTMLElement.prototype,
        "getBoundingClientRect",
      ).mockImplementation(function (this: HTMLElement) {
        if (this.getAttribute("role") === "dialog")
          return new DOMRect(8, parentTop, 298, parentTop === 8 ? 544 : 140);
        if (this.id === "nested") return new DOMRect(0, 0, 260, 320);
        return new DOMRect(20, parentTop + 50, 260, 28);
      });
      function NestedHarness() {
        const triggerRef = useRef<HTMLButtonElement>(null);
        return (
          <div className="menu-popover" role="dialog">
            <button>Project</button>
            <button ref={triggerRef}>Permission mode</button>
            <MenuPopover
              ariaLabel="Picker"
              id="nested"
              open
              role="listbox"
              triggerRef={triggerRef}
              onDismiss={() => {}}
            >
              <button role="option">Read-only</button>
            </MenuPopover>
          </div>
        );
      }
      render(<NestedHarness />);
      expect(screen.getByRole("listbox", { name: "Picker" })).toHaveStyle({
        top: expectedTop + "px",
        maxHeight: maxHeight + "px",
      });
    },
  );
  it("falls back to the viewport when neither pane space reaches the usable floor", () => {
    Object.defineProperty(window, "innerHeight", {
      configurable: true,
      value: 768,
    });
    Object.defineProperty(window, "innerWidth", {
      configurable: true,
      value: 480,
    });
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
      function (this: HTMLElement) {
        if (this.classList.contains("agent-chat-pane"))
          return new DOMRect(0, 300, 480, 100);
        if (this.getAttribute("role") === "dialog")
          return new DOMRect(8, 308, 298, 84);
        if (this.id === "constrained-picker") return new DOMRect(0, 0, 260, 80);
        return new DOMRect(20, 330, 260, 28);
      },
    );
    function ConstrainedPicker() {
      const triggerRef = useRef<HTMLButtonElement>(null);
      return (
        <section className="agent-chat-pane">
          <div className="menu-popover" role="dialog">
            <button ref={triggerRef}>Permission mode</button>
            <MenuPopover
              ariaLabel="Constrained picker"
              id="constrained-picker"
              open
              role="listbox"
              triggerRef={triggerRef}
              onDismiss={() => {}}
            >
              <button role="option">Read-only</button>
            </MenuPopover>
          </div>
        </section>
      );
    }
    render(<ConstrainedPicker />);
    expect(
      screen.getByRole("listbox", { name: "Constrained picker" }),
    ).toHaveStyle({ top: "308px", maxHeight: "320px" });
  });

  it("does not commit another render when the measured position is unchanged", () => {
    const onRender = vi.fn();
    render(
      <Profiler id="menu" onRender={onRender}>
        <MenuHarness />
      </Profiler>,
    );
    fireEvent.click(screen.getByRole("button", { name: "Actions" }));
    expect(screen.getByRole("menu", { name: "Actions" })).toBeVisible();
    onRender.mockClear();
    fireEvent.resize(window);
    fireEvent.scroll(window);
    expect(onRender).not.toHaveBeenCalled();
  });

  it("follows an unchanged-size trigger when its pane resizes and stops observing on close", () => {
    let anchor = new DOMRect(100, 100, 100, 28);
    const observers: {
      callback: ResizeObserverCallback;
      targets: Set<Element>;
    }[] = [];
    class ResizeObserverStub {
      targets = new Set<Element>();
      constructor(callback: ResizeObserverCallback) {
        observers.push({ callback, targets: this.targets });
      }
      observe(target: Element) {
        this.targets.add(target);
      }
      disconnect() {
        this.targets.clear();
      }
    }
    vi.stubGlobal("ResizeObserver", ResizeObserverStub);
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
      function (this: HTMLElement) {
        if (this.classList.contains("agent-chat-pane"))
          return new DOMRect(0, 40, 800, 700);
        return this.getAttribute("role") === "menu"
          ? new DOMRect(0, 0, 160, 40)
          : anchor;
      },
    );
    render(
      <section className="agent-chat-pane" aria-label="Chat pane">
        <MenuHarness />
      </section>,
    );
    fireEvent.click(screen.getByRole("button", { name: "Actions" }));
    const menu = screen.getByRole("menu", { name: "Actions" });
    expect(menu).toHaveStyle({ left: "40px", top: "132px" });
    const pane = screen.getByRole("region", { name: "Chat pane" });
    expect(observers.some(({ targets }) => targets.has(pane))).toBe(true);
    expect(observers.some(({ targets }) => targets.has(document.body))).toBe(
      false,
    );
    expect(
      observers.some(({ targets }) => targets.has(document.documentElement)),
    ).toBe(false);
    anchor = new DOMRect(400, 200, 100, 28);
    act(() => {
      for (const observer of observers) {
        if (observer.targets.has(pane))
          observer.callback([], {} as ResizeObserver);
      }
    });
    expect(menu).toHaveStyle({ left: "340px", top: "232px" });
    fireEvent.keyDown(menu, { key: "Escape" });
    expect(screen.queryByRole("menu", { name: "Actions" })).toBeNull();
    expect(observers.every(({ targets }) => targets.size === 0)).toBe(true);
  });

  it("repositions nested overlays without resubscribing and stops observation at the portal root", () => {
    let anchorTop = 100;
    const observers: {
      callback: ResizeObserverCallback;
      targets: Set<Element>;
      disconnect: ReturnType<typeof vi.fn>;
    }[] = [];
    class ResizeObserverStub {
      targets = new Set<Element>();
      disconnect = vi.fn(() => this.targets.clear());
      constructor(callback: ResizeObserverCallback) {
        observers.push({
          callback,
          targets: this.targets,
          disconnect: this.disconnect,
        });
      }
      observe(target: Element) {
        this.targets.add(target);
      }
    }
    vi.stubGlobal("ResizeObserver", ResizeObserverStub);
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
      function (this: HTMLElement) {
        if (this.id === "parent")
          return new DOMRect(100, anchorTop + 32, 160, 140);
        if (this.id === "child") return new DOMRect(0, 0, 160, 100);
        if (this.textContent === "Nested picker")
          return new DOMRect(120, anchorTop + 50, 100, 28);
        return new DOMRect(100, anchorTop, 160, 28);
      },
    );
    function NestedMenus() {
      const rootTrigger = useRef<HTMLButtonElement>(null);
      const childTrigger = useRef<HTMLButtonElement>(null);
      return (
        <>
          <button ref={rootTrigger}>Options</button>
          <MenuPopover
            id="parent"
            ariaLabel="Options"
            role="dialog"
            open
            triggerRef={rootTrigger}
            onDismiss={() => {}}
          >
            <button ref={childTrigger}>Nested picker</button>
            <MenuPopover
              id="child"
              ariaLabel="Nested"
              role="listbox"
              open
              triggerRef={childTrigger}
              onDismiss={() => {}}
            >
              <button role="option">Choice</button>
            </MenuPopover>
          </MenuPopover>
        </>
      );
    }
    const addListener = vi.spyOn(window, "addEventListener");
    const removeListener = vi.spyOn(window, "removeEventListener");
    const { rerender, unmount } = render(<NestedMenus />);
    const child = screen.getByRole("listbox", { name: "Nested" });
    expect(child).toHaveStyle({ top: "150px" });
    const subscriptions = observers.length;
    addListener.mockClear();
    removeListener.mockClear();
    rerender(<NestedMenus />);
    anchorTop = 200;
    fireEvent.scroll(window);
    expect(child).toHaveStyle({ top: "250px" });
    anchorTop = 300;
    act(() => {
      for (const observer of observers)
        observer.callback([], {} as ResizeObserver);
    });
    expect(child).toHaveStyle({ top: "350px" });
    expect(observers).toHaveLength(subscriptions);
    expect(
      observers.every(({ disconnect }) => disconnect.mock.calls.length === 0),
    ).toBe(true);
    expect(
      observers.some(({ targets }) => targets.has(document.documentElement)),
    ).toBe(false);
    expect(
      addListener.mock.calls.filter(
        ([type]) => type === "scroll" || type === "resize",
      ),
    ).toHaveLength(0);
    expect(
      removeListener.mock.calls.filter(
        ([type]) => type === "scroll" || type === "resize",
      ),
    ).toHaveLength(0);
    unmount();
    expect(observers.every(({ targets }) => targets.size === 0)).toBe(true);
  });

  it.each(["Escape", "Tab", "Shift+Tab"])(
    "preserves dialog interior tab stops and returns boundary %s focus to its opener",
    (key) => {
      function DialogHarness() {
        const trigger = useRef<HTMLButtonElement>(null);
        const [open, setOpen] = useState(false);
        return (
          <div>
            <button ref={trigger} onClick={() => setOpen(true)}>
              Options
            </button>
            <MenuPopover
              id="dialog"
              role="dialog"
              ariaLabel="Options dialog"
              open={open}
              triggerRef={trigger}
              onDismiss={() => setOpen(false)}
            >
              <button>First option</button>
              <button>Second option</button>
            </MenuPopover>
          </div>
        );
      }
      render(<DialogHarness />);
      const trigger = screen.getByRole("button", { name: "Options" });
      fireEvent.click(trigger);
      const dialog = screen.getByRole("dialog", { name: "Options dialog" });
      expect(dialog.parentElement).toBe(document.body);
      expect(
        screen.getByRole("button", { name: "First option" }),
      ).toHaveFocus();
      expect(
        screen.getByRole("button", { name: "Second option" }),
      ).not.toHaveAttribute("tabindex", "-1");
      const first = screen.getByRole("button", { name: "First option" });
      const last = screen.getByRole("button", { name: "Second option" });
      expect(fireEvent.keyDown(first, { key: "Tab" })).toBe(true);
      expect(dialog).toBeVisible();
      last.focus();
      expect(fireEvent.keyDown(last, { key: "Tab", shiftKey: true })).toBe(
        true,
      );
      expect(dialog).toBeVisible();
      const boundary = key === "Shift+Tab" ? first : last;
      boundary.focus();
      fireEvent.keyDown(boundary, {
        key: key === "Escape" ? key : "Tab",
        shiftKey: key === "Shift+Tab",
      });
      expect(
        screen.queryByRole("dialog", { name: "Options dialog" }),
      ).toBeNull();
      expect(trigger).toHaveFocus();
    },
  );

  it("moves nested pickers with their parent menu when the anchor moves", () => {
    let anchor = new DOMRect(100, 400, 24, 28);
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
      function (this: HTMLElement) {
        if (this.id === "outer")
          return new DOMRect(
            Number.parseFloat(this.style.left) || 0,
            Number.parseFloat(this.style.top) || 0,
            298,
            140,
          );
        if (this.id === "inner") return new DOMRect(0, 0, 260, 100);
        if (this.textContent === "Nested") {
          const parent = document.getElementById("outer")!;
          return new DOMRect(
            Number.parseFloat(parent.style.left) || 0,
            Number.parseFloat(parent.style.top) || 0,
            260,
            28,
          );
        }
        return anchor;
      },
    );
    function NestedHarness() {
      const outer = useRef<HTMLButtonElement>(null);
      const inner = useRef<HTMLButtonElement>(null);
      return (
        <>
          <button ref={outer}>Options</button>
          <MenuPopover
            ariaLabel="Outer"
            id="outer"
            open
            triggerRef={outer}
            onDismiss={() => {}}
          >
            <button ref={inner}>Nested</button>
            <MenuPopover
              ariaLabel="Inner"
              id="inner"
              open
              triggerRef={inner}
              onDismiss={() => {}}
            >
              <button role="menuitem">Choice</button>
            </MenuPopover>
          </MenuPopover>
        </>
      );
    }
    render(<NestedHarness />);
    const inner = screen.getByRole("menu", { name: "Inner" });
    const initialLeft = inner.style.left;
    anchor = new DOMRect(600, 400, 24, 28);
    fireEvent(window, new Event("resize"));
    expect(inner.style.left).not.toBe(initialLeft);
    expect(inner.style.left).toBe("628px");
  });

  it("leaves focus and keyboard ownership to an opt-in neutral popup", () => {
    function NeutralHarness() {
      const triggerRef = useRef<HTMLButtonElement>(null);
      const inputRef = useRef<HTMLInputElement>(null);
      return (
        <>
          <button ref={triggerRef} type="button">
            Trigger
          </button>
          <MenuPopover
            ariaLabel="Neutral"
            focusStrategy="none"
            id="neutral-popup"
            onDismiss={() => undefined}
            open
            role="none"
            triggerRef={triggerRef}
          >
            <input ref={inputRef} aria-label="Filter" />
          </MenuPopover>
        </>
      );
    }
    render(<NeutralHarness />);
    const input = screen.getByRole("textbox", { name: "Filter" });
    input.focus();
    fireEvent.keyDown(input, { key: "ArrowDown" });
    expect(input).toHaveFocus();
  });

  it("keeps one roving item and traps keyboard focus inside the open menu", () => {
    render(<MenuHarness />);

    fireEvent.click(screen.getByRole("button", { name: "Actions" }));
    const menu = screen.getByRole("menu", { name: "Actions" });
    const first = screen.getByRole("menuitem", { name: "First" });
    const last = screen.getByRole("menuitem", { name: "Last" });

    expect(menu).toHaveClass("menu-popover");
    expect(first).toHaveFocus();
    expect(first).toHaveAttribute("tabindex", "0");
    expect(last).toHaveAttribute("tabindex", "-1");

    fireEvent.keyDown(menu, { key: "End" });
    expect(last).toHaveFocus();
    fireEvent.keyDown(menu, { key: "Tab" });
    expect(first).toHaveFocus();
    fireEvent.keyDown(menu, { key: "Tab", shiftKey: true });
    expect(last).toHaveFocus();
  });

  it("supports reverse opening and closes Escape back onto the trigger", () => {
    render(<MenuHarness initialFocus="last" />);

    const trigger = screen.getByRole("button", { name: "Actions" });
    fireEvent.click(trigger);
    const menu = screen.getByRole("menu", { name: "Actions" });
    expect(screen.getByRole("menuitem", { name: "Last" })).toHaveFocus();

    fireEvent.keyDown(menu, { key: "Escape" });
    expect(screen.queryByRole("menu", { name: "Actions" })).toBeNull();
    expect(trigger).toHaveFocus();
  });

  it("dismisses on an outside pointer without stealing the outside focus", () => {
    render(<MenuHarness />);

    fireEvent.click(screen.getByRole("button", { name: "Actions" }));
    const outside = screen.getByRole("button", { name: "Outside" });
    outside.focus();
    fireEvent.pointerDown(outside);

    expect(screen.queryByRole("menu", { name: "Actions" })).toBeNull();
    expect(outside).toHaveFocus();
    expect(document.querySelector('[role="menu"]')).toBeNull();
  });

  it("dismisses on Tab when focus containment is not requested", () => {
    render(<MenuHarness trapFocus={false} />);

    fireEvent.click(screen.getByRole("button", { name: "Actions" }));
    expect(
      fireEvent.keyDown(screen.getByRole("menu", { name: "Actions" }), {
        key: "Tab",
      }),
    ).toBe(true);

    expect(screen.queryByRole("menu", { name: "Actions" })).toBeNull();
  });

  it("remeasures when a submenu grows near the viewport edge", () => {
    Object.defineProperty(window, "innerHeight", {
      configurable: true,
      value: 600,
    });
    const rect = (top: number, right: number, bottom: number, left: number) =>
      ({
        x: left,
        y: top,
        top,
        right,
        bottom,
        left,
        width: right - left,
        height: bottom - top,
        toJSON: () => undefined,
      }) as DOMRect;
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
      function (this: HTMLElement) {
        if (this.getAttribute("role") === "menu") {
          return this.textContent?.includes("Destination")
            ? rect(0, 220, 320, 0)
            : rect(0, 220, 40, 0);
        }
        if (this.textContent === "Grow actions") {
          return rect(450, 200, 478, 100);
        }
        return rect(0, 0, 0, 0);
      },
    );

    render(<GrowingMenuHarness />);
    fireEvent.click(screen.getByRole("button", { name: "Grow actions" }));
    const menu = screen.getByRole("menu", { name: "Grow actions" });
    expect(menu.style.top).toBe("482px");

    fireEvent.click(screen.getByRole("menuitem", { name: "Expand" }));
    expect(menu.style.top).toBe("126px");
  });

  it("rebinds closest-dialog geometry observers after the portal node moves", () => {
    const observed: HTMLElement[][] = [];
    const disconnected: boolean[] = [];
    class ResizeObserverStub {
      index = observed.length;
      constructor(_callback: ResizeObserverCallback) {
        observed.push([]);
        disconnected.push(false);
      }
      observe(element: HTMLElement) {
        observed[this.index].push(element);
      }
      disconnect() {
        disconnected[this.index] = true;
      }
      unobserve() {}
    }
    vi.stubGlobal("ResizeObserver", ResizeObserverStub);
    function DialogHarness() {
      const triggerRef = useRef<HTMLButtonElement>(null);
      return (
        <dialog open>
          <button ref={triggerRef} type="button">
            Target
          </button>
          <MenuPopover
            ariaLabel="Target"
            focusStrategy="none"
            id="dialog-popup"
            onDismiss={() => undefined}
            open
            portalTarget="closest-dialog"
            role="none"
            triggerRef={triggerRef}
          >
            <input aria-label="Target" role="combobox" />
          </MenuPopover>
        </dialog>
      );
    }
    render(<DialogHarness />);
    const popup = document.getElementById("dialog-popup");
    expect(popup).not.toBeNull();
    if (!popup) throw new Error("dialog popup missing");
    expect(popup.closest("dialog")).not.toBeNull();
    expect(observed).toHaveLength(2);
    expect(disconnected[0]).toBe(true);
    expect(observed[1]).toEqual([
      popup,
      screen.getByRole("button", { name: "Target" }),
      popup.closest("dialog"),
    ]);
  });
});

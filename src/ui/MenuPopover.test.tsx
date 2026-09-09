import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useId, useRef, useState } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { MenuPopover } from "./MenuPopover";

const INITIAL_INNER_HEIGHT = window.innerHeight;

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  Object.defineProperty(window, "innerHeight", {
    configurable: true,
    value: INITIAL_INNER_HEIGHT,
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
    fireEvent.keyDown(screen.getByRole("menu", { name: "Actions" }), {
      key: "Tab",
    });

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
    expect(observed[1]).toEqual([popup]);
  });
});

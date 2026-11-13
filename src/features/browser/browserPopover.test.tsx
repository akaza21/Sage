import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useRef } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { browserMenuKeyDown } from "./browserPopover";

afterEach(cleanup);

function Menu({ onDismiss = () => undefined }: { onDismiss?: () => void }) {
  const ref = useRef<HTMLDivElement>(null);
  return (
    <div
      ref={ref}
      role="menu"
      aria-label="More options"
      tabIndex={-1}
      onKeyDown={(event) => browserMenuKeyDown(event, ref.current, onDismiss)}
    >
      <button type="button" role="menuitem">
        Reload
      </button>
      <button type="button" role="menuitem" disabled>
        Print
      </button>
      <button type="button" role="menuitem">
        Zoom
      </button>
      <button type="button" role="menuitem">
        Clear data
      </button>
    </div>
  );
}

const item = (name: string) => screen.getByRole("menuitem", { name });

describe("browserMenuKeyDown", () => {
  it("starts from the matching end when no item has focus", () => {
    render(<Menu />);
    const menu = screen.getByRole("menu");
    menu.focus();
    fireEvent.keyDown(menu, { key: "ArrowUp" });
    expect(item("Clear data")).toHaveFocus();
    menu.focus();
    fireEvent.keyDown(menu, { key: "ArrowDown" });
    expect(item("Reload")).toHaveFocus();
  });

  it("wraps and passes over disabled items", () => {
    render(<Menu />);
    item("Reload").focus();
    fireEvent.keyDown(item("Reload"), { key: "ArrowDown" });
    expect(item("Zoom")).toHaveFocus();
    fireEvent.keyDown(item("Zoom"), { key: "End" });
    fireEvent.keyDown(item("Clear data"), { key: "ArrowDown" });
    expect(item("Reload")).toHaveFocus();
  });

  it("leaves arrows held with a modifier alone", () => {
    render(<Menu />);
    item("Reload").focus();
    expect(
      fireEvent.keyDown(item("Reload"), { key: "ArrowDown", altKey: true }),
    ).toBe(true);
    expect(item("Reload")).toHaveFocus();
  });

  it("dismisses on Escape", () => {
    const onDismiss = vi.fn();
    render(<Menu onDismiss={onDismiss} />);
    fireEvent.keyDown(item("Reload"), { key: "Escape" });
    expect(onDismiss).toHaveBeenCalledOnce();
  });
});

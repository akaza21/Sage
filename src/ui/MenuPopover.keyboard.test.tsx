import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useRef } from "react";
import { afterEach, describe, expect, it } from "vitest";

import { MenuPopover } from "./MenuPopover";

afterEach(cleanup);

function Menu({
  items,
  trapFocus,
}: {
  items: { name: string; unavailable?: boolean }[];
  trapFocus?: boolean;
}) {
  const triggerRef = useRef<HTMLButtonElement>(null);
  return (
    <>
      <button ref={triggerRef} type="button">
        Actions
      </button>
      <MenuPopover
        id="actions"
        open
        ariaLabel="Actions"
        triggerRef={triggerRef}
        trapFocus={trapFocus}
        onDismiss={() => undefined}
      >
        {items.map(({ name, unavailable }) => (
          <button
            key={name}
            type="button"
            role="menuitem"
            className="menu-item"
            aria-disabled={unavailable || undefined}
          >
            {name}
          </button>
        ))}
      </MenuPopover>
    </>
  );
}

const item = (name: string) => screen.getByRole("menuitem", { name });

describe("MenuPopover keyboard", () => {
  it("lands on the first item when Down is pressed with no item focused", () => {
    render(<Menu items={[{ name: "Copy" }, { name: "Paste" }]} />);
    const menu = screen.getByRole("menu", { name: "Actions" });
    menu.tabIndex = -1;
    menu.focus();
    fireEvent.keyDown(menu, { key: "ArrowDown" });
    expect(item("Copy")).toHaveFocus();
  });

  it("lands on the last item when Up is pressed with no item focused", () => {
    render(
      <Menu items={[{ name: "Copy" }, { name: "Paste" }, { name: "Cut" }]} />,
    );
    const menu = screen.getByRole("menu", { name: "Actions" });
    menu.tabIndex = -1;
    menu.focus();
    fireEvent.keyDown(menu, { key: "ArrowUp" });
    expect(item("Cut")).toHaveFocus();
  });

  it("traps Tab onto the first item when no item has focus", () => {
    render(<Menu trapFocus items={[{ name: "Copy" }, { name: "Paste" }]} />);
    const menu = screen.getByRole("menu", { name: "Actions" });
    menu.tabIndex = -1;
    menu.focus();
    fireEvent.keyDown(menu, { key: "Tab" });
    expect(item("Copy")).toHaveFocus();
  });

  it("keeps an unavailable item focusable so its reason can be read", () => {
    render(
      <Menu
        items={[{ name: "Copy" }, { name: "Full access", unavailable: true }]}
      />,
    );
    expect(item("Copy")).toHaveFocus();
    fireEvent.keyDown(item("Copy"), { key: "ArrowDown" });
    expect(item("Full access")).toHaveFocus();
  });

  it("leaves arrows held with a modifier to other handlers", () => {
    render(<Menu items={[{ name: "Copy" }, { name: "Paste" }]} />);
    expect(
      fireEvent.keyDown(item("Copy"), { key: "ArrowDown", shiftKey: true }),
    ).toBe(true);
    expect(item("Copy")).toHaveFocus();
  });

  it("moves to the next match when the same letter is typed again", () => {
    render(
      <Menu items={[{ name: "Copy" }, { name: "Cut" }, { name: "Close" }]} />,
    );
    expect(item("Copy")).toHaveFocus();
    fireEvent.keyDown(item("Copy"), { key: "c" });
    expect(item("Cut")).toHaveFocus();
    fireEvent.keyDown(item("Cut"), { key: "c" });
    expect(item("Close")).toHaveFocus();
  });
});

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useRef, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { afterEach, describe, expect, it, vi } from "vitest";

import { DismissLayerContext, useDismissLayer } from "./dismissLayer";

afterEach(cleanup);

/** A layer rendered into the body, like a popover or a dialog. */
function Layer({
  name,
  modal,
  onEscape,
  onOutside,
  children,
}: {
  name: string;
  modal?: boolean;
  onEscape?: () => void;
  onOutside?: () => void;
  children?: ReactNode;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const { layer, onKeyDown } = useDismissLayer(true, {
    inside: [ref],
    modal,
    onEscape,
    onOutside,
  });
  return createPortal(
    <div ref={ref} aria-label={name} role="group" onKeyDown={onKeyDown}>
      <button type="button">{`${name} item`}</button>
      <DismissLayerContext value={layer}>{children}</DismissLayerContext>
    </div>,
    document.body,
  );
}

const item = (name: string) => screen.getByRole("button", { name });

describe("useDismissLayer", () => {
  it("gives Escape to the innermost layer only, across portals", () => {
    const menu = vi.fn();
    const dialog = vi.fn();
    const pane = vi.fn();
    render(
      <div onKeyDown={pane}>
        <Layer name="menu" onEscape={menu}>
          <Layer name="dialog" modal onEscape={dialog} />
        </Layer>
      </div>,
    );
    fireEvent.keyDown(item("dialog item"), { key: "Escape" });
    expect(dialog).toHaveBeenCalledOnce();
    expect(menu).not.toHaveBeenCalled();
    expect(pane).not.toHaveBeenCalled();
  });

  it("leaves Escape that a field inside the layer took for itself", () => {
    const menu = vi.fn();
    render(
      <Layer name="menu" onEscape={menu}>
        <input
          aria-label="Rename"
          onKeyDown={(event) => event.preventDefault()}
        />
      </Layer>,
    );
    fireEvent.keyDown(screen.getByLabelText("Rename"), { key: "Escape" });
    expect(menu).not.toHaveBeenCalled();
  });

  it("ignores Escape that confirms an input method composition", () => {
    const menu = vi.fn();
    render(<Layer name="menu" onEscape={menu} />);
    fireEvent.keyDown(item("menu item"), { key: "Escape", keyCode: 229 });
    expect(menu).not.toHaveBeenCalled();
  });

  it("keeps a menu open on presses inside the dialog it opened", () => {
    const menu = vi.fn();
    const dialog = vi.fn();
    render(
      <Layer name="menu" onOutside={menu}>
        <Layer name="dialog" modal onOutside={dialog} />
      </Layer>,
    );
    fireEvent.pointerDown(item("dialog item"));
    expect(menu).not.toHaveBeenCalled();
    expect(dialog).not.toHaveBeenCalled();
  });

  it("closes only the submenu on a press inside its parent menu", () => {
    const menu = vi.fn();
    const submenu = vi.fn();
    render(
      <Layer name="menu" onOutside={menu}>
        <Layer name="submenu" onOutside={submenu} />
      </Layer>,
    );
    fireEvent.pointerDown(item("menu item"));
    expect(submenu).toHaveBeenCalledOnce();
    expect(menu).not.toHaveBeenCalled();
  });

  it("closes every layer above the topmost modal on a press outside", () => {
    const below = vi.fn();
    const menu = vi.fn();
    const submenu = vi.fn();
    render(
      <>
        <button type="button">page</button>
        <Layer name="below" onOutside={below}>
          <Layer name="dialog" modal>
            <Layer name="menu" onOutside={menu}>
              <Layer name="submenu" onOutside={submenu} />
            </Layer>
          </Layer>
        </Layer>
      </>,
    );
    fireEvent.pointerDown(screen.getByRole("button", { name: "page" }));
    expect(submenu).toHaveBeenCalledOnce();
    expect(menu).toHaveBeenCalledOnce();
    expect(below).not.toHaveBeenCalled();
  });
});

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { Splitter } from "./Splitter";

afterEach(cleanup);

function renderSplitter() {
  const onPointerResize = vi.fn();
  const onKeyResize = vi.fn();
  render(
    <Splitter
      ariaLabel="Resize panes"
      className="pane-splitter"
      activeClassName="pane-splitter--active"
      value={40}
      min={20}
      max={80}
      onPointerResize={onPointerResize}
      onKeyResize={onKeyResize}
    />,
  );
  return {
    splitter: screen.getByRole("separator", { name: "Resize panes" }),
    onPointerResize,
    onKeyResize,
  };
}

describe("Splitter", () => {
  it("describes itself as a vertical separator with its range", () => {
    const { splitter } = renderSplitter();
    expect(splitter).toHaveAttribute("aria-orientation", "vertical");
    expect(splitter).toHaveAttribute("aria-valuemin", "20");
    expect(splitter).toHaveAttribute("aria-valuemax", "80");
    expect(splitter).toHaveAttribute("aria-valuenow", "40");
    expect(splitter).toHaveAttribute("tabindex", "0");
  });

  it("resizes from the press and every move until release", () => {
    const { splitter, onPointerResize } = renderSplitter();
    fireEvent.pointerDown(splitter, { pointerId: 1, clientX: 100 });
    expect(splitter).toHaveClass("pane-splitter--active");
    fireEvent.pointerMove(splitter, { pointerId: 1, clientX: 120 });
    fireEvent.pointerUp(splitter, { pointerId: 1, clientX: 120 });
    fireEvent.pointerMove(splitter, { pointerId: 1, clientX: 140 });
    expect(onPointerResize.mock.calls.map(([x]) => x)).toEqual([100, 120]);
    expect(onPointerResize).toHaveBeenCalledWith(100, splitter);
    expect(splitter).not.toHaveClass("pane-splitter--active");
  });

  it.each(["pointerCancel", "lostPointerCapture"] as const)(
    "ends the drag on %s, so a later hover does not resize",
    (end) => {
      const { splitter, onPointerResize } = renderSplitter();
      fireEvent.pointerDown(splitter, { pointerId: 1, clientX: 100 });
      fireEvent[end](splitter, { pointerId: 1 });
      fireEvent.pointerMove(splitter, { pointerId: 1, clientX: 140 });
      expect(onPointerResize).toHaveBeenCalledTimes(1);
      expect(splitter).not.toHaveClass("pane-splitter--active");
    },
  );

  it("maps the arrows, Home and End, with Shift for a large step", () => {
    const { splitter, onKeyResize } = renderSplitter();
    for (const key of ["ArrowLeft", "ArrowRight", "Home", "End"])
      expect(fireEvent.keyDown(splitter, { key })).toBe(false);
    fireEvent.keyDown(splitter, { key: "ArrowRight", shiftKey: true });
    expect(onKeyResize.mock.calls).toEqual([
      ["ArrowLeft", false],
      ["ArrowRight", false],
      ["Home", false],
      ["End", false],
      ["ArrowRight", true],
    ]);
  });

  it("leaves other keys and modified arrows to app shortcuts", () => {
    const { splitter, onKeyResize } = renderSplitter();
    expect(fireEvent.keyDown(splitter, { key: "ArrowUp" })).toBe(true);
    for (const modifier of ["ctrlKey", "altKey", "metaKey"])
      expect(
        fireEvent.keyDown(splitter, { key: "ArrowLeft", [modifier]: true }),
      ).toBe(true);
    expect(onKeyResize).not.toHaveBeenCalled();
  });
});

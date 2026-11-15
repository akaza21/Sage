import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { WorkbenchSidebarSplitter } from "./WorkbenchSidebarSplitter";
import { resizeWorkbenchSidebar } from "./workbenchSidebarResize";

const WIDTH = { defaultWidth: 360, minWidth: 280, maxWidth: 720 };

afterEach(cleanup);

describe("WorkbenchSidebarSplitter", () => {
  it("keeps the trailing artifact splitter semantic without synthetic hit-target metadata", () => {
    render(
      <WorkbenchSidebarSplitter
        ariaLabel="Resize files and preview"
        side="trailing"
        value={360}
        width={{ defaultWidth: 360, minWidth: 280, maxWidth: 720 }}
        onChange={vi.fn()}
      />,
    );

    const splitter = screen.getByRole("separator", {
      name: "Resize files and preview",
    });
    expect(splitter).toHaveClass("workbench-sidebar-splitter--trailing");
    expect(splitter).toHaveAttribute("aria-orientation", "vertical");
    expect(splitter).toHaveAttribute("aria-valuemin", "280");
    expect(splitter).toHaveAttribute("aria-valuemax", "720");
    expect(splitter).not.toHaveAttribute("data-hit-target");
  });

  it("retains trailing keyboard width semantics", () => {
    const onChange = vi.fn();
    render(
      <WorkbenchSidebarSplitter
        ariaLabel="Resize files and preview"
        side="trailing"
        value={360}
        width={{ defaultWidth: 360, minWidth: 280, maxWidth: 720 }}
        onChange={onChange}
      />,
    );

    const splitter = screen.getByRole("separator", {
      name: "Resize files and preview",
    });
    fireEvent.keyDown(splitter, { key: "ArrowLeft" });
    fireEvent.keyDown(splitter, { key: "ArrowRight" });
    expect(onChange.mock.calls).toEqual([
      [
        resizeWorkbenchSidebar(WIDTH, {
          currentWidth: 360,
          key: "ArrowRight",
          shiftKey: false,
        }),
      ],
      [
        resizeWorkbenchSidebar(WIDTH, {
          currentWidth: 360,
          key: "ArrowLeft",
          shiftKey: false,
        }),
      ],
    ]);
    expect(onChange.mock.calls[0][0]).toBeGreaterThan(360);
  });

  it("measures the pointer from the near edge of the parent", () => {
    const onChange = vi.fn();
    const renderSide = (side: "leading" | "trailing") =>
      render(
        <div data-testid={side}>
          <WorkbenchSidebarSplitter
            ariaLabel={side}
            side={side}
            value={360}
            width={WIDTH}
            pointerOffsetAdjustment={side === "leading" ? 40 : 0}
            onChange={onChange}
          />
        </div>,
      );
    renderSide("leading");
    renderSide("trailing");
    for (const side of ["leading", "trailing"]) {
      vi.spyOn(
        screen.getByTestId(side),
        "getBoundingClientRect",
      ).mockReturnValue({ left: 100, right: 1100 } as DOMRect);
    }

    fireEvent.pointerDown(screen.getByRole("separator", { name: "leading" }), {
      clientX: 500,
    });
    fireEvent.pointerDown(screen.getByRole("separator", { name: "trailing" }), {
      clientX: 500,
    });
    expect(onChange.mock.calls).toEqual([
      [resizeWorkbenchSidebar(WIDTH, { pointerOffset: 500 - 100 - 40 })],
      [resizeWorkbenchSidebar(WIDTH, { pointerOffset: 1100 - 500 })],
    ]);
  });
});

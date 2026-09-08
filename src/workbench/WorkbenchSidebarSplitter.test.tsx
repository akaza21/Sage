import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { WorkbenchSidebarSplitter } from "./WorkbenchSidebarSplitter";

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

    fireEvent.keyDown(
      screen.getByRole("separator", { name: "Resize files and preview" }),
      { key: "ArrowLeft" },
    );
    expect(onChange).toHaveBeenCalledTimes(1);
  });
});

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { WorkbenchTab } from "../../domain/workbench";
import { ChatSessionStrip } from "./ChatSessionStrip";

const tabs: WorkbenchTab[] = [
  {
    id: "chat:design",
    kind: "chat",
    title: "Chat: Design",
    position: 0,
    pinned: false,
    dirty: false,
  },
  {
    id: "chat:implementation",
    kind: "chat",
    title: "Chat: Implementation",
    position: 1,
    pinned: false,
    dirty: false,
  },
];

afterEach(cleanup);

describe("ChatSessionStrip", () => {
  it("renders accessible named tabs and dispatches selection, creation, and close", () => {
    const onSelect = vi.fn();
    const onClose = vi.fn();
    const onCreate = vi.fn();
    render(
      <ChatSessionStrip
        tabs={tabs}
        selectedId="chat:design"
        onSelect={onSelect}
        onClose={onClose}
        onCreate={onCreate}
      />,
    );

    const tablist = screen.getByRole("tablist", { name: "Chat tabs" });
    expect(tablist).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "Design" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    fireEvent.click(screen.getByRole("tab", { name: "Implementation" }));
    fireEvent.click(
      screen.getByRole("button", { name: "Archive chat Design" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "New agent chat" }));
    expect(onSelect).toHaveBeenCalledWith("chat:implementation");
    expect(onClose).toHaveBeenCalledWith("chat:design");
    expect(onCreate).toHaveBeenCalledTimes(1);
  });

  it("moves the roving tab focus with arrow keys", () => {
    const onSelect = vi.fn();
    render(
      <ChatSessionStrip
        tabs={tabs}
        selectedId="chat:design"
        onSelect={onSelect}
        onClose={() => undefined}
        onCreate={() => undefined}
        disabled={false}
      />,
    );

    const design = screen.getByRole("tab", { name: "Design" });
    fireEvent.keyDown(design, { key: "ArrowRight" });
    expect(onSelect).not.toHaveBeenCalled();
    expect(screen.getByRole("tab", { name: "Implementation" })).toHaveFocus();
    fireEvent.keyDown(screen.getByRole("tab", { name: "Implementation" }), {
      key: "Enter",
    });
    expect(onSelect).toHaveBeenCalledWith("chat:implementation");
  });
});

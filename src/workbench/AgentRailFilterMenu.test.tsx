import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  agentRailViewPrefs,
  resetAgentRailViewPrefs,
} from "./agentRailViewPrefs";
import { AgentRailFilterMenu } from "./AgentRailFilterMenu";

afterEach(() => {
  cleanup();
  resetAgentRailViewPrefs();
});

describe("AgentRailFilterMenu", () => {
  it("uses the shared menu lifecycle and roving keyboard focus", async () => {
    render(
      <>
        <AgentRailFilterMenu onMarkAllRead={vi.fn()} />
        <button type="button">Outside</button>
      </>,
    );

    const trigger = screen.getByRole("button", { name: "Chat list options" });
    fireEvent.click(trigger);

    const menu = screen.getByRole("menu", { name: "Chat list options" });
    expect(screen.getByRole("group", { name: "Grouping" })).toBeVisible();
    expect(screen.getByRole("group", { name: "Ordering" })).toBeVisible();
    expect(screen.getByRole("group", { name: "Show" })).toBeVisible();
    expect(screen.getByRole("group", { name: "Filters" })).toBeVisible();
    const folder = screen.getByRole("menuitemradio", { name: "Folder" });
    const status = screen.getByRole("menuitemradio", { name: "Status" });
    expect(menu).toHaveClass("menu-popover", "agent-rail-filter__menu");
    expect(menu.parentElement).toBe(document.body);
    expect(folder).toHaveFocus();
    expect(folder).toHaveAttribute("tabindex", "0");
    expect(status).toHaveAttribute("tabindex", "-1");

    fireEvent.keyDown(menu, { key: "ArrowDown" });
    expect(status).toHaveFocus();
    fireEvent.keyDown(menu, { key: "End" });
    expect(
      screen.getByRole("menuitem", { name: "Mark All as Read" }),
    ).toHaveFocus();
    fireEvent.keyDown(menu, { key: "Escape" });
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
    expect(trigger).toHaveFocus();

    fireEvent.keyDown(trigger, { key: "ArrowUp" });
    expect(
      screen.getByRole("menuitem", { name: "Mark All as Read" }),
    ).toHaveFocus();

    const outside = screen.getByRole("button", { name: "Outside" });
    outside.focus();
    fireEvent.pointerDown(outside);
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
    expect(outside).toHaveFocus();
  });

  it("keeps status filters inside the menu's radio interaction model", () => {
    render(<AgentRailFilterMenu onMarkAllRead={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: "Chat list options" }));

    const all = screen.getByRole("menuitemradio", {
      name: "All statuses",
    });
    const working = screen.getByRole("menuitemradio", { name: "Working" });
    expect(all).toHaveAttribute("aria-checked", "true");
    expect(working).toHaveAttribute("aria-checked", "false");

    fireEvent.click(working);
    expect(agentRailViewPrefs().statusFilter).toBe("working");
    expect(all).toHaveAttribute("aria-checked", "false");
    expect(working).toHaveAttribute("aria-checked", "true");
  });

  it("runs terminal actions once, closes, and restores trigger focus", async () => {
    const onMarkAllRead = vi.fn();
    render(<AgentRailFilterMenu onMarkAllRead={onMarkAllRead} />);

    const trigger = screen.getByRole("button", { name: "Chat list options" });
    fireEvent.click(trigger);
    fireEvent.click(screen.getByRole("menuitem", { name: "Mark All as Read" }));

    expect(onMarkAllRead).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
    await waitFor(() => expect(trigger).toHaveFocus());
  });
});

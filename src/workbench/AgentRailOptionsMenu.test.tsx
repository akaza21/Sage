import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import {
  AgentPrimaryNavigation,
  AgentRailOptionsMenu,
} from "./AgentRailOptionsMenu";
import {
  agentRailSectionCollapsed,
  agentRailViewPrefs,
  resetAgentRailViewPrefs,
  toggleAgentRailSection,
} from "./agentRailViewPrefs";

afterEach(() => {
  cleanup();
  resetAgentRailViewPrefs();
});

it("keeps expanded navigation free of duplicate search and settings controls", () => {
  render(<AgentPrimaryNavigation />);
  expect(screen.getByRole("button", { name: "Home" })).toBeVisible();
  expect(screen.getAllByRole("button")).toHaveLength(3);
  expect(screen.queryByRole("button", { name: "Search chats" })).toBeNull();
});

it("expands each section before delegating collapsed-rail navigation", () => {
  toggleAgentRailSection("recent");
  toggleAgentRailSection("projects");
  const onNavigate = vi.fn((target: "Recent" | "Projects") => {
    expect(agentRailSectionCollapsed(target.toLowerCase())).toBe(false);
  });
  render(<AgentPrimaryNavigation onNavigate={onNavigate} />);
  fireEvent.click(screen.getByRole("button", { name: "Go to recent chats" }));
  fireEvent.click(screen.getByRole("button", { name: "Go to projects" }));
  expect(onNavigate.mock.calls).toEqual([["Recent"], ["Projects"]]);
});

it("retains search and settings actions for the collapsed rail", () => {
  const onSearch = vi.fn();
  const onSettings = vi.fn();
  const mounted = render(
    <AgentPrimaryNavigation
      onSearch={onSearch}
      onSettings={onSettings}
      searchDisabled
    />,
  );
  const search = screen.getByRole("button", { name: "Search chats" });
  expect(search).toBeDisabled();
  fireEvent.click(search);
  expect(onSearch).not.toHaveBeenCalled();
  mounted.rerender(
    <AgentPrimaryNavigation
      onSearch={onSearch}
      onSettings={onSettings}
      searchDisabled={false}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: "Search chats" }));
  fireEvent.click(
    screen.getByRole("button", { name: "Open settings from rail" }),
  );
  expect(onSearch).toHaveBeenCalledOnce();
  expect(onSettings).toHaveBeenCalledOnce();
});

it("keeps a submenu open when a hover is followed by a click", async () => {
  render(<AgentRailOptionsMenu />);
  fireEvent.click(screen.getByRole("button", { name: "Sidebar options" }));
  const organize = await screen.findByRole("menuitem", {
    name: "Organize sidebar",
  });
  fireEvent.pointerEnter(organize);
  fireEvent.click(organize);
  expect(organize).toHaveAttribute("aria-expanded", "true");
  fireEvent.click(
    await screen.findByRole("menuitemradio", { name: "In one list" }),
  );
  expect(agentRailViewPrefs().organize).toBe("list");
});

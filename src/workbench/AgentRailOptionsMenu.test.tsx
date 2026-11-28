import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { AgentRailOptionsMenu } from "./AgentRailOptionsMenu";
import {
  agentRailViewPrefs,
  resetAgentRailViewPrefs,
} from "./agentRailViewPrefs";

afterEach(() => {
  cleanup();
  resetAgentRailViewPrefs();
});

it("keeps a submenu open when a hover is followed by a click", async () => {
  render(<AgentRailOptionsMenu />);
  fireEvent.click(screen.getByRole("button", { name: "Sidebar options" }));
  const organize = await screen.findByRole("menuitem", {
    name: "Organize sidebar",
  });
  fireEvent.pointerEnter(organize);
  fireEvent.pointerMove(organize);
  fireEvent.click(organize);
  expect(organize).toHaveAttribute("aria-expanded", "true");
  fireEvent.click(
    await screen.findByRole("menuitemradio", { name: "In one list" }),
  );
  expect(agentRailViewPrefs().organize).toBe("list");
});

async function openOptions() {
  render(<AgentRailOptionsMenu />);
  fireEvent.click(screen.getByRole("button", { name: "Sidebar options" }));
  return {
    organize: await screen.findByRole("menuitem", { name: "Organize sidebar" }),
    sort: screen.getByRole("menuitem", { name: "Sort chats by" }),
  };
}

function hover(item: HTMLElement, clientX: number, clientY: number) {
  fireEvent.pointerEnter(item, { clientX, clientY });
  fireEvent.pointerMove(item, { clientX, clientY });
}

function placeSubmenu(name: string) {
  const submenu = screen.getByRole("menu", { name });
  vi.spyOn(submenu, "getBoundingClientRect").mockReturnValue(
    DOMRect.fromRect({ x: 200, y: 0, width: 160, height: 80 }),
  );
}

it("keeps a submenu open while the pointer heads to it across the next item", async () => {
  const { organize, sort } = await openOptions();
  hover(organize, 150, 10);
  expect(organize).toHaveAttribute("aria-expanded", "true");
  placeSubmenu("Organize sidebar");
  fireEvent.pointerLeave(organize, { clientX: 160, clientY: 20 });
  hover(sort, 180, 30);
  expect(organize).toHaveAttribute("aria-expanded", "true");
  expect(sort).toHaveAttribute("aria-expanded", "false");
});

it("opens the next submenu once the pointer turns away from the open one", async () => {
  const { organize, sort } = await openOptions();
  hover(organize, 150, 10);
  placeSubmenu("Organize sidebar");
  fireEvent.pointerLeave(organize, { clientX: 150, clientY: 20 });
  hover(sort, 100, 40);
  expect(sort).toHaveAttribute("aria-expanded", "true");
  expect(organize).toHaveAttribute("aria-expanded", "false");
});

it("opens with ArrowRight and closes with ArrowLeft back onto its item", async () => {
  const { organize } = await openOptions();
  organize.focus();
  fireEvent.keyDown(organize, { key: "ArrowRight" });
  const current = await screen.findByRole("menuitemradio", {
    name: "By project",
  });
  expect(organize).toHaveAttribute(
    "aria-controls",
    screen.getByRole("menu", { name: "Organize sidebar" }).id,
  );
  fireEvent.keyDown(current, { key: "ArrowLeft" });
  expect(organize).toHaveAttribute("aria-expanded", "false");
  expect(organize).toHaveFocus();
});

it("stays closed after Escape while the pointer rests on its item", async () => {
  const { organize } = await openOptions();
  hover(organize, 150, 10);
  const current = await screen.findByRole("menuitemradio", {
    name: "By project",
  });
  fireEvent.keyDown(current, { key: "Escape" });
  expect(organize).toHaveAttribute("aria-expanded", "false");
  fireEvent.pointerMove(organize, { clientX: 151, clientY: 10 });
  expect(organize).toHaveAttribute("aria-expanded", "false");
  expect(organize).toHaveFocus();
  hover(organize, 152, 10);
  expect(organize).toHaveAttribute("aria-expanded", "true");
});

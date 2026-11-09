import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import type * as LazyModule from "./agentTaskHeaderLazy";
import type * as MenuModule from "./ProjectChatActionMenu";

// The menu body's chunk arrives only when a test lets it, or fails on cue.
// Each test mocks it afresh, so each starts with the body not loaded.
const chunk = vi.hoisted(() => {
  let failures = 0;
  let release = () => {};
  let held = Promise.resolve();
  return {
    hold() {
      held = new Promise<void>((resolve) => {
        release = resolve;
      });
    },
    release: () => release(),
    failNext() {
      failures += 1;
    },
    async arrive() {
      await held;
      if (failures > 0) {
        failures -= 1;
        throw new Error("The menu chunk failed to load.");
      }
    },
  };
});

let AgentHeaderMore: typeof MenuModule.AgentHeaderMore;
let preloadAgentHeaderMenu: typeof LazyModule.preloadAgentHeaderMenu;

beforeEach(async () => {
  vi.resetModules();
  chunk.hold();
  vi.doMock("./AgentHeaderMenu", async (importOriginal) => {
    await chunk.arrive();
    return importOriginal();
  });
  ({ AgentHeaderMore } = await import("./ProjectChatActionMenu"));
  ({ preloadAgentHeaderMenu } = await import("./agentTaskHeaderLazy"));
});
afterEach(() => {
  chunk.release();
  cleanup();
});

const TRIGGER = "Chat actions for Task task";

function renderMenu() {
  render(
    <>
      <AgentHeaderMore
        scope="chat-a"
        title="Task"
        projectName="Sage"
        disabled={false}
        onChanges={() => undefined}
      >
        <button type="button" role="menuitem">
          Project actions
        </button>
      </AgentHeaderMore>
      <input aria-label="Elsewhere" />
    </>,
  );
  return screen.getByRole("button", { name: TRIGGER });
}

function openMenu() {
  const trigger = renderMenu();
  trigger.focus();
  fireEvent.click(trigger);
  expect(trigger).toHaveAttribute("aria-expanded", "true");
  return trigger;
}

async function arrive() {
  chunk.release();
  await act(() => preloadAgentHeaderMenu());
}

it("closes on Escape before its body has loaded, and the late body stays shut", async () => {
  const trigger = openMenu();
  fireEvent.keyDown(trigger, { key: "Escape" });
  expect(trigger).toHaveAttribute("aria-expanded", "false");
  expect(trigger).toHaveFocus();
  await arrive();
  expect(screen.queryByRole("menu")).toBeNull();
  expect(trigger).toHaveFocus();
});

it("names the menu it controls only once that menu is there", async () => {
  const trigger = openMenu();
  expect(trigger).not.toHaveAttribute("aria-controls");
  await arrive();
  const menu = screen.getByRole("menu", { name: "Chat tools" });
  expect(trigger).toHaveAttribute("aria-controls", menu.id);
});

it("closes on Tab before its body has loaded", () => {
  const trigger = openMenu();
  fireEvent.keyDown(trigger, { key: "Tab" });
  expect(trigger).toHaveAttribute("aria-expanded", "false");
});

it("closes on a press outside before its body has loaded", () => {
  const trigger = openMenu();
  fireEvent.pointerDown(trigger);
  expect(trigger).toHaveAttribute("aria-expanded", "true");
  fireEvent.pointerDown(screen.getByRole("textbox", { name: "Elsewhere" }));
  expect(trigger).toHaveAttribute("aria-expanded", "false");
});

it("leaves focus where it moved while the body loaded", async () => {
  openMenu();
  const elsewhere = screen.getByRole("textbox", { name: "Elsewhere" });
  elsewhere.focus();
  await arrive();
  expect(screen.getByRole("menuitem", { name: "Changes" })).toBeVisible();
  expect(elsewhere).toHaveFocus();
});

it("closes with a notice when the body fails to load, then retries on the next open", async () => {
  chunk.failNext();
  const trigger = openMenu();
  chunk.release();
  await waitFor(() =>
    expect(trigger).toHaveAttribute("aria-expanded", "false"),
  );
  expect(screen.getByRole("alert")).toHaveTextContent(
    "Chat tools could not be loaded.",
  );
  fireEvent.click(trigger);
  expect(screen.queryByRole("alert")).toBeNull();
  const first = await screen.findByRole("menuitem", {
    name: "Project actions",
  });
  await waitFor(() => expect(first).toHaveFocus());
});

it("keeps the header when an early preload fails", async () => {
  chunk.failNext();
  const trigger = renderMenu();
  fireEvent.pointerEnter(trigger);
  chunk.release();
  await act(() => preloadAgentHeaderMenu().catch(() => undefined));
  expect(trigger).toBeInTheDocument();
  expect(trigger).not.toHaveAttribute("aria-controls");
  expect(screen.queryByRole("alert")).toBeNull();
  fireEvent.click(trigger);
  expect(
    await screen.findByRole("menuitem", { name: "Changes" }),
  ).toBeInTheDocument();
});

import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, beforeAll, expect, it, vi } from "vitest";
import { preloadAgentHeaderMenu } from "./agentTaskHeaderLazy";

// The header menu body loads when the window first idles.
beforeAll(() => preloadAgentHeaderMenu());
import { useWorkbenchStore } from "../app/store";
import type { WorkbenchTab } from "../domain/workbench";
import { AgentTaskHeader } from "./AgentTaskHeader";
import type * as ResourcesChunk from "./AgentTaskResources";
// Only the resources chunk is held cold here. The rename menu's chunk loads
// with the file, so opening it never races a cold import against findByRole.
import "./ProjectChatActionPopover";

const gate = vi.hoisted(() => {
  let finish!: () => void;
  const wait = new Promise<void>((resolve) => {
    finish = resolve;
  });
  return { wait, finish };
});
vi.mock("./AgentTaskResources", async (original) => {
  await gate.wait;
  const chunk = await original<typeof ResourcesChunk>();
  return { ...chunk, AgentTaskResources: vi.fn(chunk.AgentTaskResources) };
});

const initialState = useWorkbenchStore.getState();
afterEach(() => {
  cleanup();
  useWorkbenchStore.setState(initialState, true);
});

it("cancels cold resources and rename on outside pointer, Escape and Tab before loading can steal focus", async () => {
  const tab: WorkbenchTab = {
    id: "chat:pending",
    kind: "chat",
    title: "Pending header",
    position: 0,
    pinned: false,
    dirty: false,
  };
  useWorkbenchStore.setState({
    project: { id: "pending-project", name: "Sage", roots: [] },
    tabs: [tab],
    activeTabId: tab.id,
  });
  const mounted = render(
    <>
      <button>Composer</button>
      <AgentTaskHeader tab={tab} chatCount={1} snapshot={null} />
    </>,
  );
  const outside = screen.getByRole("button", { name: "Composer" });
  const resources = screen.getByRole("button", { name: "Show chat resources" });
  const actions = screen.getByRole("button", {
    name: "Chat actions for Pending header task",
  });
  const openRename = async () => {
    fireEvent.click(actions);
    fireEvent.click(
      screen.getByRole("menuitem", { name: "More chat actions in Sage" }),
    );
    fireEvent.click(
      await screen.findByRole("menuitem", { name: "Rename chat" }),
    );
  };
  resources.focus();
  fireEvent.keyDown(resources, { key: "Tab" });
  for (const key of ["Escape", "Tab", null]) {
    resources.focus();
    fireEvent.click(resources);
    expect(resources).toHaveAttribute("aria-expanded", "true");
    if (key) fireEvent.keyDown(resources, { key });
    else {
      outside.focus();
      fireEvent.pointerDown(outside);
    }
    expect(resources).toHaveAttribute("aria-expanded", "false");
    await openRename();
    expect(screen.getByText("Pending header")).toBeVisible();
    expect(resources).toHaveFocus();
    if (key) fireEvent.keyDown(resources, { key });
    else {
      outside.focus();
      fireEvent.pointerDown(outside);
    }
    expect(actions).not.toBeDisabled();
  }
  await openRename();
  mounted.rerender(
    <>
      <button>Composer</button>
      <AgentTaskHeader
        tab={{ ...tab, id: "chat:replacement" }}
        chatCount={1}
        snapshot={null}
      />
    </>,
  );
  outside.focus();
  await act(async () => {
    gate.finish();
    await import("./AgentTaskResources");
  });
  expect(outside).toHaveFocus();
  const chunk = await import("./AgentTaskResources");
  expect(chunk.AgentTaskResources).not.toHaveBeenCalled();
  expect(screen.queryByRole("dialog", { name: "Chat resources" })).toBeNull();
  expect(screen.queryByRole("textbox", { name: "New chat name" })).toBeNull();
  fireEvent.click(resources);
  expect(
    await screen.findByRole("dialog", { name: "Chat resources" }),
  ).toBeVisible();
});

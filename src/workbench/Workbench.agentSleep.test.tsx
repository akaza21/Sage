import { useEffect } from "react";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../app/store";
import { settleLazyModules } from "../test/settleLazyModules";
import { Workbench } from "./Workbench";
import { writeWorkbenchLayout } from "./workbenchLayoutStorage";

const body = vi.hoisted(() => ({ renders: 0, liveTimers: 0 }));

vi.mock("./AgentWorkbenchBody", () => ({
  AgentWorkbenchBody: () => {
    body.renders += 1;
    useEffect(() => {
      body.liveTimers += 1;
      const timer = window.setInterval(() => undefined, 60_000);
      return () => {
        body.liveTimers -= 1;
        window.clearInterval(timer);
      };
    }, []);
    return <section aria-label="Agent body probe" />;
  },
}));
vi.mock("./ActiveEditorCanvas", () => ({ default: () => null }));
vi.mock("./DirtyTabDialog", () => ({ DirtyTabDialog: () => null }));
vi.mock("./RightSidebar", () => ({ RightSidebar: () => null }));
vi.mock("./StatusBar", () => ({ StatusBar: () => null }));
vi.mock("./TabStrip", () => ({ TabStrip: () => null }));
vi.mock("./useCompactWorkbenchLayout", () => ({
  useCompactWorkbenchLayout: () => false,
}));

const initialState = useWorkbenchStore.getState();

beforeEach(() => {
  body.renders = 0;
  body.liveTimers = 0;
});

afterEach(() => {
  cleanup();
  useWorkbenchStore.setState(initialState, true);
});

async function selectMode(name: "Agent" | "Editor") {
  fireEvent.click(screen.getByRole("radio", { name }));
  await settleLazyModules();
}

it("mounts the Agent body only on its first Agent visit", async () => {
  writeWorkbenchLayout({ windowMode: "editor" });
  render(<Workbench />);
  await settleLazyModules();
  expect(screen.queryByLabelText("Agent body probe")).toBeNull();
  expect(body.renders).toBe(0);

  await selectMode("Agent");
  expect(screen.getByLabelText("Agent body probe")).toBeVisible();
});

it("keeps one Agent body asleep in Editor without re-rendering it", async () => {
  writeWorkbenchLayout({ windowMode: "agent" });
  render(<Workbench />);
  await settleLazyModules();
  const node = screen.getByLabelText("Agent body probe");
  expect(body.liveTimers).toBe(1);
  const rendersBeforeToggle = body.renders;

  await selectMode("Editor");
  expect(node.isConnected).toBe(true);
  expect(node).not.toBeVisible();
  expect(body.liveTimers).toBe(0);

  await selectMode("Agent");
  expect(screen.getAllByLabelText("Agent body probe")).toEqual([node]);
  expect(node).toBeVisible();
  expect(body.liveTimers).toBe(1);
  expect(body.renders).toBe(rendersBeforeToggle);
});

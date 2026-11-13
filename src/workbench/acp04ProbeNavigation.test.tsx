import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import {
  createAcp04Ui,
  freshChatReadyExpression,
} from "../../scripts/source-functional-acp04-live-probe.mjs";
import { useWorkbenchStore } from "../app/store";
import { settleLazyModules } from "../test/settleLazyModules";
import { Workbench } from "./Workbench";

// The ACP-04 journey drives the packaged app through these expressions, so
// they run here against the real Agent rail rather than a markup copy.
vi.mock("../features/chat/ChatSurface", () => ({
  ChatSurface: () => <textarea aria-label="Message agent" />,
}));

const initialState = useWorkbenchStore.getState();
const seeded = {
  id: "chat:seeded",
  kind: "chat" as const,
  title: "Seeded chat",
  position: 0,
  pinned: false,
  dirty: false,
};
const ui = createAcp04Ui("codex 0.167.0");
const evaluate = (expression: string): unknown =>
  // The probe sends these strings to the page through CDP Runtime.evaluate.
  window.eval(expression);

beforeEach(() => {
  localStorage.clear();
  vi.stubGlobal("innerWidth", 1800);
  vi.stubGlobal(
    "matchMedia",
    vi.fn(() => ({
      matches: false,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    })),
  );
  useWorkbenchStore.setState({
    ...initialState,
    phase: "ready",
    project: { id: "project-a", name: "Sage", roots: [] },
    projects: [
      { id: "project-a", name: "Sage", rootCount: 0, lastOpenedAt: 1 },
    ],
    tabs: [seeded],
    activeTabId: seeded.id,
    sidebarOpen: true,
    createChat: vi.fn(async () => {
      useWorkbenchStore.setState({
        tabs: [seeded, { ...seeded, id: "chat:fresh", title: "New chat" }],
        activeTabId: "chat:fresh",
      });
      return { status: "confirmed" as const, chatId: "chat:fresh" };
    }),
  });
});

afterEach(() => {
  cleanup();
  localStorage.clear();
  vi.unstubAllGlobals();
  useWorkbenchStore.setState(initialState, true);
});

it("opens an unsaved draft through the live probe's New chat action", async () => {
  render(<Workbench />);
  await settleLazyModules();

  expect(evaluate(ui.seededChatId)).toBe(seeded.id);
  expect(evaluate(freshChatReadyExpression(seeded.id))).toBe(false);
  expect(evaluate(ui.createFreshChat)).toBe(true);
  await settleLazyModules();

  expect(
    screen.getByRole("heading", { name: "What should we work on?" }),
  ).toBeVisible();
  expect(screen.getByRole("combobox", { name: "Message agent" })).toHaveFocus();
  expect(useWorkbenchStore.getState().tabs).toEqual([seeded]);
  expect(evaluate(freshChatReadyExpression(seeded.id))).toBe(false);
});

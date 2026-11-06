import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../../app/store";
import { AgentSettingsSurface } from "./AgentSettingsSurface";
import { useAgentSettingsState } from "./agentSettingsState";

// The Harnesses module arrives only when the test releases it, like a cold
// import on a slow disk.
const gate = vi.hoisted(() => {
  let release!: () => void;
  const ready = new Promise<void>((resolve) => (release = resolve));
  return { ready, release: () => release() };
});
vi.mock("./AgentHarnessSettings", async (importOriginal) => {
  await gate.ready;
  return importOriginal();
});

const workbench = useWorkbenchStore.getState();

beforeEach(() => {
  useAgentSettingsState.getState().open("Chats");
  useWorkbenchStore.setState({
    project: { id: "project-a", name: "A", roots: [] },
    projectTransitioning: false,
    harnessDiscoveryPhase: "ready",
    harnesses: [],
  });
});

afterEach(() => {
  cleanup();
  useWorkbenchStore.setState(workbench, true);
});

it("keeps the current page on screen until the Harnesses page is ready, then focuses it", async () => {
  render(<AgentSettingsSurface />);
  const section = screen.getByRole("button", { name: "Harnesses" });
  section.focus();
  fireEvent.click(section);
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 20));
  });
  // Still Chats: the heading and its page stay while the module loads.
  expect(
    screen.getByRole("heading", { level: 2, name: "Chats" }),
  ).toBeVisible();
  expect(screen.queryByText(/Loading/)).toBeNull();
  expect(section).toHaveAttribute("aria-current", "page");

  gate.release();
  const refresh = await screen.findByRole("button", {
    name: "Refresh discovery",
  });
  expect(
    screen.getByRole("heading", { level: 2, name: "Harnesses" }),
  ).toBeVisible();
  expect(screen.queryByText(/Loading/)).toBeNull();
  // Focus moves from the section button into the page that just arrived.
  expect(refresh).toHaveFocus();
});

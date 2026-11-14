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

it("leaves focus and typed text in Search when the user moves there while Harnesses loads", async () => {
  render(<AgentSettingsSurface />);
  const section = screen.getByRole("button", { name: "Harnesses" });
  section.focus();
  fireEvent.click(section);
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 20));
  });
  const search = screen.getByRole("searchbox", {
    name: "Search Agent settings sections",
  });
  search.focus();
  fireEvent.change(search, { target: { value: "har" } });

  gate.release();
  await screen.findByRole("button", { name: "Refresh discovery" });
  expect(
    screen.getByRole("heading", { level: 2, name: "Harnesses" }),
  ).toBeVisible();
  expect(search).toHaveFocus();
  expect(search).toHaveValue("har");
});

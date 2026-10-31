import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { AgentSettingsSurface } from "./AgentSettingsSurface";
import { useAgentSettingsState } from "./agentSettingsState";

const execution = vi.hoisted(() => ({ fail: null as (() => void) | null }));
vi.mock(
  "./AgentSettingsExecution",
  () =>
    new Promise((_resolve, reject) => {
      execution.fail = () => reject(new Error("Execution chunk unavailable"));
    }),
);

afterEach(() => {
  cleanup();
  useAgentSettingsState.getState().close();
  vi.restoreAllMocks();
});

it("keeps headless execution loading and failures out of Settings grid placement", async () => {
  vi.spyOn(console, "error").mockImplementation(() => undefined);
  useAgentSettingsState.getState().open("Chats");
  render(<AgentSettingsSurface />);
  const settings = screen.getByRole("region", { name: "Agent settings" });
  await waitFor(() => expect(execution.fail).not.toBeNull());
  expect(settings.children).toHaveLength(2);
  expect(settings.children[0]).toHaveAttribute(
    "aria-label",
    "Agent settings sections",
  );
  expect(screen.queryByText(/Loading Chat execution/)).toBeNull();
  await act(async () => execution.fail?.());
  const error = await screen.findByRole("alert", {
    name: "Chat execution failed to load",
  });
  expect(error.parentElement).toHaveClass("agent-settings__content");
  expect(settings.children).toHaveLength(2);
  expect(
    screen.getByRole("combobox", { name: "Current harness" }),
  ).toBeVisible();
});

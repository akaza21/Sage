import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { isTauriRuntime } from "../../ports/nativeInvoke";
import { nativeNotificationPort } from "../../ports/nativeNotificationPort";
import AgentNotificationSettings from "./AgentNotificationSettings";

vi.mock("../../ports/nativeInvoke", () => ({
  isTauriRuntime: vi.fn(() => true),
}));
vi.mock("../../ports/nativeNotificationPort", () => ({
  nativeNotificationPort: {
    getPreference: vi.fn(),
    setPreference: vi.fn(),
  },
}));

const port = vi.mocked(nativeNotificationPort);
const control = () =>
  screen.getByRole<HTMLSelectElement>("combobox", {
    name: "Turn notifications",
  });

beforeEach(() => {
  vi.mocked(isTauriRuntime).mockReturnValue(true);
  port.getPreference.mockResolvedValue("off");
  port.setPreference.mockImplementation(async (value) => value);
});
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

it("shows the saved preference and turns notices on", async () => {
  render(<AgentNotificationSettings />);
  await waitFor(() => expect(control().value).toBe("off"));
  fireEvent.change(control(), { target: { value: "backgroundTurns" } });
  await waitFor(() => expect(control().value).toBe("backgroundTurns"));
  expect(control().disabled).toBe(false);
});

it("keeps the saved preference when saving fails", async () => {
  port.setPreference.mockRejectedValue(
    new Error("notification preference could not be saved"),
  );
  render(<AgentNotificationSettings />);
  await waitFor(() => expect(control().value).toBe("off"));
  fireEvent.change(control(), { target: { value: "backgroundTurns" } });
  expect(
    await screen.findByText("notification preference could not be saved"),
  ).toBeTruthy();
  expect(control().value).toBe("off");
});

it("does not offer a native preference outside the desktop app", () => {
  vi.mocked(isTauriRuntime).mockReturnValue(false);
  render(<AgentNotificationSettings />);
  expect(screen.queryByRole("combobox")).toBeNull();
});

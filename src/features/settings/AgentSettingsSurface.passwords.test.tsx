import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type * as NativeInvoke from "../../ports/nativeInvoke";
import type * as NativePasswordsPort from "../../ports/nativePasswordsPort";
import { AgentSettingsSurface } from "./AgentSettingsSurface";
import { useAgentSettingsState } from "./agentSettingsState";

const native = vi.hoisted(() => ({
  overview: vi.fn(),
  reveal: vi.fn(),
}));

vi.mock("../../ports/nativeInvoke", async (importOriginal) => ({
  ...(await importOriginal<typeof NativeInvoke>()),
  isTauriRuntime: () => true,
}));
vi.mock("../../ports/nativePasswordsPort", async (importOriginal) => ({
  ...(await importOriginal<typeof NativePasswordsPort>()),
  passwordsOverview: (query: string) => native.overview(query),
  passwordsReveal: (id: string) => native.reveal(id),
}));

let visibility: DocumentVisibilityState = "visible";

function setDocumentVisibility(next: DocumentVisibilityState) {
  visibility = next;
  document.dispatchEvent(new Event("visibilitychange"));
}

beforeEach(() => {
  visibility = "visible";
  vi.spyOn(document, "visibilityState", "get").mockImplementation(
    () => visibility,
  );
  native.overview.mockResolvedValue({
    status: "ready",
    total: 1,
    matching: 1,
    entries: [{ id: "a1", origin: "https://example.com", username: "ada" }],
  });
});

afterEach(() => {
  cleanup();
  useAgentSettingsState.getState().close();
  vi.clearAllMocks();
  vi.restoreAllMocks();
});

it("forgets revealed and typed passwords when the window is minimized", async () => {
  useAgentSettingsState.getState().open("Passwords");
  render(<AgentSettingsSurface />);
  const row = await screen.findByRole("group", { name: "ada on example.com" });
  native.reveal.mockResolvedValueOnce("shown-secret");
  await act(async () => {
    fireEvent.click(within(row).getByRole("button", { name: "Show" }));
  });
  expect(screen.getByText("shown-secret")).toBeVisible();

  await act(async () => setDocumentVisibility("hidden"));
  expect(screen.queryByText("shown-secret")).toBeNull();
  await act(async () => setDocumentVisibility("visible"));
  expect(screen.queryByText("shown-secret")).toBeNull();

  fireEvent.click(
    within(screen.getByRole("group", { name: "ada on example.com" })).getByRole(
      "button",
      { name: "Edit" },
    ),
  );
  const password = screen.getByLabelText("New password");
  fireEvent.change(password, { target: { value: "draft-secret" } });
  await act(async () => setDocumentVisibility("hidden"));
  expect(password).toHaveValue("");
  await act(async () => setDocumentVisibility("visible"));
  expect(screen.getByLabelText("New password")).toHaveValue("");
});

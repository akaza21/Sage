import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { useWorkbenchStore } from "../../app/store";
import type { HarnessDefinition } from "../../domain/harness";
import { getWorkbenchPorts } from "../../ports/workbenchRuntime";
import { useAgentSettingsState } from "../settings/agentSettingsState";
import { ComposerHarnessMenuExtras } from "./ComposerHarnessMenuExtras";

const initial = useWorkbenchStore.getState();
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  useWorkbenchStore.setState(initial, true);
});

const claude: HarnessDefinition = {
  id: "claude-code",
  name: "Claude Code",
  description: "",
  source: "builtIn",
  accountModelDiscovery: false,
  transport: "acp",
  installState: "available",
  bindingId: "claude-code",
};

function withHarnesses() {
  useWorkbenchStore.setState({
    harnesses: [
      claude,
      {
        ...claude,
        id: "pi",
        name: "Pi",
        brand: "pi",
        installState: "notInstalled",
        bindingId: null,
      } as HarnessDefinition,
    ],
  });
}

it("lists harnesses this computer lacks with Install, which opens Settings' harness page", () => {
  withHarnesses();
  const open = vi.fn();
  vi.spyOn(useAgentSettingsState, "getState").mockReturnValue({
    ...useAgentSettingsState.getState(),
    open,
  });
  render(<ComposerHarnessMenuExtras />);
  const missing = screen.getByRole("group", { name: "Not installed" });
  expect(missing).toHaveTextContent("PiInstall");
  expect(missing.querySelector("[data-harness-icon]")).toHaveAttribute(
    "data-harness-icon",
    "pi",
  );
  fireEvent.click(screen.getByRole("button", { name: "Install Pi" }));
  expect(open).toHaveBeenCalledWith("Harnesses");
});

it("runs discovery again and applies the new inventory", async () => {
  withHarnesses();
  const refreshed: HarnessDefinition[] = [claude];
  vi.spyOn(getWorkbenchPorts().agent, "refreshHarnesses").mockResolvedValue(
    refreshed,
  );
  const apply = vi.fn(async (harnesses: typeof refreshed) => {
    useWorkbenchStore.setState({ harnesses });
  });
  useWorkbenchStore.setState({ applyHarnessDiscovery: apply });
  render(<ComposerHarnessMenuExtras />);
  const refresh = screen.getByRole("button", { name: "Refresh harnesses" });
  fireEvent.click(refresh);
  expect(refresh).toHaveTextContent("Checking harnesses…");
  expect(refresh).toBeDisabled();
  await waitFor(() => expect(apply).toHaveBeenCalledWith(refreshed));
  expect(screen.queryByRole("group", { name: "Not installed" })).toBeNull();
  expect(
    screen.getByRole("button", { name: "Refresh harnesses" }),
  ).toBeEnabled();
});

it("reports a failed refresh in the menu", async () => {
  vi.spyOn(getWorkbenchPorts().agent, "refreshHarnesses").mockRejectedValue(
    new Error("Discovery is unavailable."),
  );
  render(<ComposerHarnessMenuExtras />);
  fireEvent.click(screen.getByRole("button", { name: "Refresh harnesses" }));
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "Discovery is unavailable.",
  );
});

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { settleLazyModules } from "../../test/settleLazyModules";
import { ComposerHarnessPicker } from "../chat/ComposerHarnessPicker";
import { useWorkbenchStore } from "../../app/store";
import { ChatModelPicker } from "../chat/ChatModelPicker";

vi.mock("../../ports/desktopRuntime", () => ({ isTauriRuntime: () => true }));
vi.mock("../../services/acpDesktop", () => ({
  acpDesktop: { getClaudeAuthStatus: async () => ({ loggedIn: false }) },
}));
const initial = useWorkbenchStore.getState();
afterEach(() => {
  cleanup();
  useWorkbenchStore.setState(initial, true);
});

it("keeps model choices free of harness sign-in status", () => {
  useWorkbenchStore.setState({
    harnessAccount: "signedOut",
    harnessDiscoveryPhase: "signInRequired",
  });
  render(
    <ChatModelPicker
      listboxId="models"
      models={[
        { id: "sonnet", name: "Sonnet", group: "Claude Code" },
        { id: "gpt", name: "GPT", group: "Codex" },
        { id: "pi-model", name: "Pi model", group: "Pi" },
      ]}
      selectedId="gpt"
      disabled={false}
      onSelect={() => undefined}
    />,
  );
  expect(screen.getAllByRole("option").map((row) => row.textContent)).toEqual([
    "Sonnet",
    "GPT",
    "Pi model",
  ]);
  expect(
    screen.queryByText(/signed in|signed out|checking sign-in/i),
  ).toBeNull();
});

it("shows harness choices and availability without sign-in text or actions", async () => {
  useWorkbenchStore.setState({
    harnesses: [
      {
        id: "claude-code",
        name: "Claude Code",
        description: "",
        source: "builtIn",
        installState: "available",
        transport: "acp",
        bindingId: "claude",
        accountModelDiscovery: false,
      },
      {
        id: "codex",
        name: "Codex",
        description: "",
        source: "builtIn",
        installState: "available",
        transport: "acp",
        bindingId: "account",
        accountModelDiscovery: true,
      },
    ],
    harnessAccount: "signedOut",
    harnessDiscoveryPhase: "signInRequired",
  });
  render(
    <ComposerHarnessPicker
      candidates={[
        { id: "claude-code", label: "Claude Code" },
        { id: "codex", label: "Codex" },
        {
          id: "pi",
          label: "Pi",
          subtitle: "Not installed",
          disabledReason: "Install Pi first",
        },
      ]}
      selectedId="codex"
      onSelect={() => undefined}
      open
    />,
  );
  await settleLazyModules();
  expect(screen.getByRole("option", { name: "Claude Code" })).toBeVisible();
  expect(screen.getByRole("option", { name: "Codex" })).toBeVisible();
  expect(screen.getByText("Not installed")).toBeVisible();

  expect(
    screen.queryByText(
      /signed in|signed out|checking sign-in|status unavailable/i,
    ),
  ).toBeNull();
  expect(screen.queryByRole("button", { name: /sign in/i })).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Harness" }));
});

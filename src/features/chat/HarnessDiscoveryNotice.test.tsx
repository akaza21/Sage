import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { useWorkbenchStore } from "../../app/store";
import { HarnessDiscoveryNotice } from "./HarnessDiscoveryNotice";
import { SettingsHarnessNotices } from "../settings/SettingsHarnessNotices";

const initialState = useWorkbenchStore.getState();

afterEach(() => {
  cleanup();
  useWorkbenchStore.setState(initialState, true);
});

describe("HarnessDiscoveryNotice", () => {
  it("keeps Settings compatibility and update copy in its existing presentation", () => {
    const { container } = render(
      <HarnessDiscoveryNotice
        presentation={SettingsHarnessNotices}
        harness={{
          id: "fixture",
          name: "Fixture",
          description: "ACP",
          source: "builtIn",
          transport: "acp",
          installState: "available",
          bindingId: "fixture-binding",
          accountModelDiscovery: false,
          characterisation: "newerThanTested",
          boundVersion: "1.2.3",
          update: { latestVersion: "1.3.0", command: "update fixture" },
        }}
      />,
    );
    expect(screen.getByRole("status")).toHaveTextContent(
      "Fixture 1.2.3 is newer than Sage has tested. Tested features stay on. Fixture 1.3.0 is available. Run update fixture, then refresh discovery in Settings.",
    );
    expect(container.querySelector(".build-notice")).not.toBeNull();
    expect(container.querySelector(".notice")).toBeNull();
  });
  it("explains that a present Codex harness remains blocked pending admission", () => {
    useWorkbenchStore.setState({
      harnesses: [
        {
          id: "codex",
          name: "Codex",
          description: "ACP",
          installState: "unavailable",
          source: "builtIn",
          accountModelDiscovery: true,
          transport: "acp",
          bindingId: "codex-app-server",
        },
      ],
      selectedHarnessId: "codex",
      harnessDiscoveryPhase: "idle",
      harnessDiscoveryError: null,
    });

    render(<HarnessDiscoveryNotice />);

    expect(screen.getByRole("alert")).toHaveTextContent(
      "Codex can't start yet.",
    );
  });

  it("shows why Sage runs the adapter's Claude Code apart from the update", () => {
    useWorkbenchStore.setState({
      harnesses: [
        {
          id: "claude-code",
          name: "Claude Code",
          description: "ACP",
          installState: "available",
          source: "builtIn",
          accountModelDiscovery: false,
          transport: "acp",
          bindingId: "claude-agent-acp",
          version: "2.1.250",
          boundVersion: "0.76.0",
          characterisation: "characterised",
          runsVersion: "2.1.257",
          directFromVersion: "2.1.300",
          update: {
            latestVersion: "2.1.330",
            command: "npm install -g @anthropic-ai/claude-code@latest",
          },
        },
      ],
      selectedHarnessId: "claude-code",
      harnessDiscoveryPhase: "idle",
    });

    const { container } = render(<HarnessDiscoveryNotice />);

    const notices = [...container.querySelectorAll(".notice")];
    expect(notices.map((notice) => notice.textContent)).toEqual([
      expect.stringContaining(
        "Sage runs Claude Code 2.1.257 bundled with its adapter. Update Claude Code to 2.1.300 or newer and Sage runs it directly.",
      ),
      expect.stringContaining("Claude Code 2.1.330 is available."),
    ]);
  });

  it("names the harness and bound build when Sage has not characterised it", () => {
    useWorkbenchStore.setState({
      harnesses: [
        {
          id: "claude-code",
          name: "Claude Code",
          description: "ACP",
          installState: "available",
          source: "builtIn",
          accountModelDiscovery: false,
          transport: "acp",
          bindingId: "claude-agent-acp",
          boundVersion: "0.99.0",
          characterisation: "uncharacterised",
        },
      ],
      selectedHarnessId: "claude-code",
      harnessDiscoveryPhase: "idle",
      harnessDiscoveryError: null,
    });

    render(<HarnessDiscoveryNotice />);

    expect(screen.getByText(/Claude Code 0.99.0/)).toHaveTextContent(
      "Claude Code 0.99.0 hasn't been tested with Sage yet, so some features are off.",
    );
    expect(screen.getByText(/some features are off\./)).toBeVisible();
  });

  it("stays silent once the bound build is characterised", () => {
    useWorkbenchStore.setState({
      harnesses: [
        {
          id: "claude-code",
          name: "Claude Code",
          description: "ACP",
          installState: "available",
          source: "builtIn",
          accountModelDiscovery: false,
          transport: "acp",
          bindingId: "claude-agent-acp",
          boundVersion: "0.76.0",
          characterisation: "characterised",
        },
      ],
      selectedHarnessId: "claude-code",
      harnessDiscoveryPhase: "idle",
      harnessDiscoveryError: null,
    });

    render(<HarnessDiscoveryNotice />);

    expect(screen.queryByRole("status")).toBeNull();
  });

  it("does not keep an account-connected banner in the chat after discovery", () => {
    useWorkbenchStore.setState({
      harnesses: [
        {
          id: "codex",
          name: "Codex",
          description: "ACP",
          installState: "available",
          source: "builtIn",
          accountModelDiscovery: true,
          transport: "acp",
          bindingId: "codex-app-server",
          characterisation: "characterised",
        },
      ],
      selectedHarnessId: "codex",
      harnessDiscoveryPhase: "ready",
      harnessDiscoveryError: null,
    });

    render(<HarnessDiscoveryNotice />);

    expect(screen.queryByText("Codex account connected.")).toBeNull();
    expect(screen.queryByRole("button", { name: "Sign out" })).toBeNull();
  });

  it("names the specific refusal instead of a bare unavailable state", () => {
    useWorkbenchStore.setState({
      harnesses: [
        {
          id: "claude-code",
          name: "Claude Code",
          description: "ACP",
          installState: "unavailable",
          source: "builtIn",
          accountModelDiscovery: false,
          transport: "acp",
          bindingId: "claude-agent-acp",
          unavailableReason: "nodeEngineMismatch",
        },
      ],
      selectedHarnessId: "claude-code",
      harnessDiscoveryPhase: "idle",
      harnessDiscoveryError: null,
    });

    render(<HarnessDiscoveryNotice />);

    expect(screen.getByRole("alert")).toHaveTextContent(
      "Claude Code needs a newer Node.js version.",
    );
  });

  it("falls back safely when the backend emits an unknown refusal token", () => {
    useWorkbenchStore.setState({
      harnesses: [
        {
          id: "claude-code",
          name: "Claude Code",
          description: "ACP",
          installState: "unavailable",
          source: "builtIn",
          accountModelDiscovery: false,
          transport: "acp",
          bindingId: "claude-agent-acp",
          unavailableReason: "someFutureReason" as never,
        },
      ],
      selectedHarnessId: "claude-code",
      harnessDiscoveryPhase: "idle",
      harnessDiscoveryError: null,
    });

    render(<HarnessDiscoveryNotice />);

    expect(screen.getByRole("alert")).toHaveTextContent(
      "Claude Code can't start yet.",
    );
    expect(screen.queryByText("someFutureReason")).toBeNull();
  });

  it("uses one blocking notice when an account-managed harness is unavailable", () => {
    useWorkbenchStore.setState({
      harnesses: [
        {
          id: "codex",
          name: "Codex",
          description: "ACP",
          installState: "unavailable",
          source: "builtIn",
          accountModelDiscovery: true,
          transport: "acp",
          bindingId: "codex-app-server",
          unavailableReason: "admissionIncomplete",
        },
      ],
      selectedHarnessId: "codex",
      harnessDiscoveryPhase: "idle",
      harnessDiscoveryError: null,
    });

    render(<HarnessDiscoveryNotice />);

    expect(screen.getAllByRole("alert")).toHaveLength(1);
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Codex is still being checked.",
    );
  });
});

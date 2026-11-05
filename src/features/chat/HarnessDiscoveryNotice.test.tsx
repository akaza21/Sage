import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { useWorkbenchStore } from "../../app/store";
import { HarnessDiscoveryNotice } from "./HarnessDiscoveryNotice";

const initialState = useWorkbenchStore.getState();

afterEach(() => {
  cleanup();
  useWorkbenchStore.setState(initialState, true);
});

describe("HarnessDiscoveryNotice", () => {
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

    expect(screen.getByRole("status")).toHaveTextContent(
      "Sage did not admit this harness and will not start it.",
    );
  });

  it("names the bound build when Sage has not characterised it", () => {
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

    expect(screen.getByRole("status")).toHaveTextContent(
      "Build 0.99.0 hasn't been characterised. Some features are unavailable.",
    );
    expect(screen.getByText(/Some features are unavailable\./)).toBeVisible();
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

    expect(screen.getByRole("status")).toHaveTextContent(
      "The installed Node.js is older than this harness requires.",
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

    expect(screen.getByRole("status")).toHaveTextContent(
      "Sage did not admit this harness and will not start it.",
    );
    expect(screen.queryByText("someFutureReason")).toBeNull();
  });

  it("uses one status region when an account-managed harness is unavailable", () => {
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

    expect(screen.getAllByRole("status")).toHaveLength(1);
    expect(screen.getByRole("status")).toHaveTextContent(
      "This harness has not completed admission",
    );
  });
});

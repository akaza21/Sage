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
      "Codex was found but is not admitted. Sage will not start it.",
    );
    expect(screen.getByText(/managed package and state roots/i)).toBeVisible();
  });
});

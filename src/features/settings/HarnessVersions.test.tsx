import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import type { HarnessDefinition } from "../../domain/harness";
import { HarnessVersionDetails } from "./HarnessVersions";

afterEach(cleanup);

const claude: HarnessDefinition = {
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
  runsVersion: "2.1.257",
  directFromVersion: "2.1.300",
  latestVersion: "2.1.330",
  update: {
    latestVersion: "2.1.330",
    command: "npm install -g @anthropic-ai/claude-code@latest",
    unlocks: ["reload-plugins"],
  },
};

describe("HarnessVersionDetails", () => {
  it("explains the Claude fallback and lists what the update adds", () => {
    render(<HarnessVersionDetails harness={claude} />);
    expect(
      screen.getByText("Installed 2.1.250 · Sage runs 2.1.257 · Latest 2.1.330"),
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        "Sage runs the 2.1.257 release bundled with its adapter, because 2.1.250 is older than 2.1.300. Update Claude Code to 2.1.300 or newer and Sage runs it directly.",
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("list", { name: "Claude Code 2.1.330 adds" }),
    ).toHaveTextContent("/reload-plugins");
  });

  it("shows the installed release as what Sage runs, and no list without a check", () => {
    render(
      <HarnessVersionDetails
        harness={{
          ...claude,
          runsVersion: null,
          directFromVersion: null,
          latestVersion: null,
          update: null,
        }}
      />,
    );
    expect(
      screen.getByText(
        "Installed 2.1.250 · Sage runs 2.1.250 · Latest not checked",
      ),
    ).toBeInTheDocument();
    expect(screen.queryByRole("list")).toBeNull();
  });
});

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { ChatLegacyTranscript } from "./ChatLegacyTranscript";

import { useWorkbenchStore } from "../../app/store";
const initial = useWorkbenchStore.getState();

afterEach(() => {
  cleanup();
  useWorkbenchStore.setState(initial, true);
  vi.unstubAllEnvs();
});

it.each([
  [true, true],
  [true, false],
  [false, true],
  [false, false],
])(
  "shows the exposed simulator intro independently of DEV: %s, %s",
  (developmentBuild, developmentSimulator) => {
    vi.stubEnv("DEV", developmentBuild);
    useWorkbenchStore.setState({
      turnChanges: [
        {
          resource: { rootId: "root", relativePath: "edited.ts" },
          indexStatus: "modified",
          worktreeStatus: "modified",
          additions: 1,
          deletions: 0,
        },
      ],
    });
    render(
      <ChatLegacyTranscript
        developmentSimulator={developmentSimulator}
        messages={[
          {
            id: "history",
            sessionId: "session",
            sequence: 1,
            role: "assistant",
            text: "Retained history",
            createdAt: 0,
          },
        ]}
      />,
    );
    expect(screen.getByText("Retained history")).toBeVisible();
    expect(Boolean(screen.queryByText("Development simulator"))).toBe(
      developmentSimulator,
    );
    expect(
      Boolean(
        screen.queryByRole("region", { name: "Files edited in this turn" }),
      ),
    ).toBe(developmentBuild && developmentSimulator);
  },
);

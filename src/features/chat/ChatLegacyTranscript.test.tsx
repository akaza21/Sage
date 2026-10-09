import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { ChatLegacyTranscript } from "./ChatLegacyTranscript";

vi.mock("./EditSummary", () => ({ EditSummary: () => <p>Simulator edits</p> }));

afterEach(() => {
  cleanup();
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
    expect(Boolean(screen.queryByText("Simulator edits"))).toBe(
      developmentBuild && developmentSimulator,
    );
  },
);

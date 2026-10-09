import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { ChatLegacyTranscript } from "./ChatLegacyTranscript";

afterEach(() => {
  cleanup();
  vi.unstubAllEnvs();
});

it("keeps legacy history but excludes simulator chrome from production", () => {
  vi.stubEnv("DEV", false);
  render(
    <ChatLegacyTranscript
      developmentSimulator
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
  expect(screen.queryByText("Development simulator")).toBeNull();
});

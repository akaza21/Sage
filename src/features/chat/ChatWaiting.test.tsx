import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { ChatTranscriptTurnSummary } from "./ChatTranscriptFooter";
import { page } from "./acp/acpProjection.fixture";

afterEach(cleanup);
it("distinguishes startup, first response and actual activity without retaining a waiting row", () => {
  const turn = page("sending").turns[0];
  const { rerender } = render(
    <ChatTranscriptTurnSummary turn={turn} hasActivities={false} />,
  );
  expect(screen.getByRole("status")).toHaveTextContent("Starting agent");
  rerender(
    <ChatTranscriptTurnSummary
      turn={{ ...turn, state: "running" }}
      hasActivities={false}
    />,
  );
  expect(screen.getByRole("status")).toHaveTextContent("Waiting for response");
  rerender(
    <ChatTranscriptTurnSummary
      turn={{ ...turn, state: "running" }}
      hasActivities
    >
      Actual tool
    </ChatTranscriptTurnSummary>,
  );
  expect(screen.queryByText(/Waiting for response/)).toBeNull();
  expect(screen.getByText("Actual tool")).toBeVisible();
  rerender(
    <ChatTranscriptTurnSummary
      turn={{ ...turn, state: "completed" }}
      hasActivities={false}
    />,
  );
  expect(screen.queryByText(/Waiting for response|Starting agent/)).toBeNull();
});

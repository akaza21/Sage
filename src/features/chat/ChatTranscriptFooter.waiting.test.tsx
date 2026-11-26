import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";

import { page } from "./acp/acpProjection.fixture";
import { ChatTranscriptTurnSummary } from "./ChatTranscriptFooter";

afterEach(cleanup);

it("says the turn waits on the user instead of counting it as working", () => {
  const turn = { ...page("waitingApproval").turns[0], createdAt: Date.now() - 167_000 };
  const { container } = render(
    <ChatTranscriptTurnSummary turn={turn} hasActivities />,
  );
  expect(
    container.querySelector(".chat-transcript-work__heading"),
  ).toHaveTextContent("Waiting for approval");
  expect(screen.queryByText(/Working for/)).toBeNull();
});

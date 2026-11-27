import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";

import { AcpChatPresentation } from "./acp/AcpChatPresentation";
import { page } from "./acp/acpProjection.fixture";
import { ChatTranscriptTurnSummary } from "./ChatTranscriptFooter";
import { conversationState } from "./conversationActionAvailability";

afterEach(cleanup);

it("says the turn waits on the user instead of counting it as working", () => {
  const turn = {
    ...page("waitingApproval").turns[0],
    createdAt: Date.now() - 167_000,
  };
  const { container } = render(
    <ChatTranscriptTurnSummary turn={turn} hasActivities />,
  );
  expect(
    container.querySelector(".chat-transcript-work__heading"),
  ).toHaveTextContent("Waiting for approval");
  expect(screen.queryByText(/Working for/)).toBeNull();
});

it("keeps a settled turn working while a background task outlives it", () => {
  const settled = page("completed");
  const running = {
    ...settled,
    sessionSurface: {
      activities: [
        {
          key: "a".repeat(24),
          kind: "backgroundTask",
          label: "npm run dev",
          state: "running",
        },
      ],
    },
  };
  render(
    <AcpChatPresentation
      page={running}
      hasEarlier={false}
      loadingEarlier={false}
      historyError={null}
      projectRoots={[]}
      retentionIncomplete={false}
    />,
  );
  expect(screen.getByText("Working in the background")).toBeVisible();
  expect(screen.queryByText(/Worked for/)).toBeNull();
  // Rewind and fork wait for it too.
  expect(conversationState(running).busy).toBe(true);
  expect(conversationState(settled).busy).toBe(false);
});

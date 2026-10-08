import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import type { AcpChatPage } from "../domain/acp";
import { SubagentSurface } from "./SubagentSurface";

afterEach(cleanup);

describe("SubagentSurface", () => {
  it("does not call a mixed running/completed group done", () => {
    const page = {
      turns: [
        {
          items: [
            {
              id: "spawn",
              kind: "collaboration",
              payload: {
                receiverLabels: ["A", "B"],
                agentKeys: ["a", "b"],
                agentStates: [
                  ["a", "completed"],
                  ["b", "running"],
                ],
              },
            },
          ],
          activities: [],
        },
      ],
    } as unknown as AcpChatPage;
    render(<SubagentSurface page={page} itemId="spawn" />);
    expect(screen.getByRole("status")).toHaveTextContent(
      "Working when observed",
    );
  });
  it("shows only bounded, parented activity and does not imply a full transcript", () => {
    const page = {
      turns: [
        {
          items: [
            {
              id: "spawn",
              kind: "collaboration",
              payload: {
                receiverLabels: ["Reviewer"],
                agentKeys: ["secret"],
                agentStates: [["secret", "running"]],
              },
            },
            {
              id: "child",
              kind: "toolCall",
              activityId: "activity-child",
              lifecycle: "completed",
              payload: { presentation: { label: "Review files" } },
            },
            {
              id: "unrelated",
              kind: "toolCall",
              activityId: "activity-other",
              lifecycle: "completed",
              payload: { presentation: { label: "Unrelated work" } },
            },
          ],
          activities: [
            {
              id: "activity-child",
              parentItemId: "spawn",
              parentActivityId: null,
            },
          ],
        },
      ],
    } as unknown as AcpChatPage;
    render(<SubagentSurface page={page} itemId="spawn" />);
    expect(
      screen.getByRole("heading", { name: "Reviewer" }),
    ).toBeInTheDocument();
    expect(screen.getByText("Review files")).toBeInTheDocument();
    expect(screen.queryByText("Unrelated work")).toBeNull();
    expect(
      screen.getByText(/full child transcript is not available/),
    ).toBeInTheDocument();
    expect(screen.queryByText("secret")).toBeNull();
  });
});

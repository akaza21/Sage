import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import type { AcpChatPage } from "../domain/acp";
import { SubagentSurface } from "./SubagentSurface";

afterEach(cleanup);

describe("SubagentSurface", () => {
  it("reopens a completed child with its bounded recorded activity", () => {
    const page = {
      chatId: "chat",
      projectId: "project",
      turns: [
        {
          id: "turn",
          sessionId: "session",
          browserOperations: [],
          items: [
            {
              id: "spawn",
              kind: "collaboration",
              payload: {
                operation: "spawnAgent",
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
              id: "finish",
              kind: "collaboration",
              payload: {
                operation: "wait",
                receiverLabels: ["Reviewer"],
                agentKeys: ["secret"],
                agentStates: [["secret", "completed"]],
              },
            },
          ],
          activities: [{ id: "activity-child", parentItemId: "spawn" }],
        },
      ],
    } as unknown as AcpChatPage;
    render(<SubagentSurface page={page} itemId="spawn" agentKey="secret" />);
    expect(screen.getByRole("status")).toHaveTextContent("Done");
    expect(screen.getByText("Review files")).toBeVisible();
    expect(screen.getByText(/Bounded activity recorded/)).toBeVisible();
  });
  it("reconciles an attributed started row with its later completion", () => {
    const page = {
      projectId: "project",
      chatId: "chat",
      turns: [
        {
          id: "turn",
          sessionId: "session",
          browserOperations: [],
          items: [
            {
              id: "spawn",
              kind: "collaboration",
              payload: {
                operation: "spawnAgent",
                receiverLabels: ["Reviewer"],
                agentKeys: ["secret"],
                agentStates: [["secret", "pendingInit"]],
              },
            },
            {
              id: "started",
              kind: "collaboration",
              payload: {
                operation: "started",
                agentKey: "secret",
                receiverLabels: ["Reviewer"],
                agentKeys: ["secret"],
                agentStates: [["secret", "running"]],
              },
            },
            {
              id: "finish",
              kind: "collaboration",
              payload: {
                operation: "wait",
                receiverLabels: ["Reviewer"],
                agentKeys: ["secret"],
                agentStates: [["secret", "completed"]],
              },
            },
          ],
          activities: [],
        },
      ],
    } as unknown as AcpChatPage;
    render(<SubagentSurface page={page} itemId="started" agentKey="secret" />);
    expect(screen.getByRole("status")).toHaveTextContent("Done");
  });
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

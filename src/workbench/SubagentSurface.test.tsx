import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { AcpChatPage } from "../domain/acp";
import { SubagentSurface } from "./SubagentSurface";

const readChild = vi.hoisted(() => vi.fn());
vi.mock("../ports/nativeInvoke", async (importOriginal) => ({
  ...((await importOriginal()) as object),
  invokeNative: readChild,
}));

afterEach(cleanup);

describe("SubagentSurface", () => {
  it.each(["session", "invocation"])(
    "does not relabel a selected native child when its key is reused by another %s",
    (scope) => {
      readChild.mockClear();
      const identity = {
        projectId: "project",
        chatId: "chat",
        sessionId: "session-a",
      };
      const spawn = (id: string, name: string, sessionId = "session-a") => ({
        ...identity,
        id,
        sessionId,
        kind: "collaboration",
        payload: {
          operation: "spawnAgent",
          receiverLabels: [name],
          agentKeys: ["private-reused"],
          agentStates: [["private-reused", "running"]],
        },
      });
      const native = (id: string, operation: string, label: string) => ({
        ...identity,
        id,
        kind: "collaboration",
        payload: {
          operation,
          label,
          receiverLabels: [],
          agentKey: "private-reused",
          agentKeys: [],
          agentStates: [],
        },
      });
      const first = {
        id: "turn-a",
        sessionId: "session-a",
        activities: [],
        items: [
          spawn("spawn-a", "Reviewer"),
          native("started-a", "started", "Reviewer"),
        ],
      };
      const second = {
        id: "turn-b",
        sessionId: "session-b",
        activities: [],
        items: [spawn("spawn-b", "Builder", "session-b")],
      };
      const page = {
        ...identity,
        currentSessionId: scope === "session" ? "session-b" : "session-a",
        sessions: [],
        turns:
          scope === "session"
            ? [first, second]
            : [
                {
                  ...first,
                  items: [
                    ...first.items,
                    native("done-a", "completed", "Subagent"),
                    spawn("spawn-b", "Builder"),
                  ],
                },
              ],
      } as unknown as AcpChatPage;
      render(
        <SubagentSurface
          page={page}
          itemId="started-a"
          agentKey="private-reused"
        />,
      );
      expect(screen.getByRole("heading", { name: "Reviewer" })).toBeVisible();
      if (scope === "session")
        expect(screen.getByText(/from an earlier session/)).toBeVisible();
      else expect(screen.getByRole("status")).toHaveTextContent("Done");
      expect(readChild).not.toHaveBeenCalled();
    },
  );
  it("preserves the retained name and done state when opening a bound terminal activity", () => {
    readChild.mockClear();
    const identity = {
      projectId: "project",
      chatId: "chat",
      sessionId: "session",
    };
    const page = {
      ...identity,
      currentSessionId: "session",
      sessions: [],
      turns: [
        {
          id: "turn",
          sessionId: "session",
          items: [
            {
              ...identity,
              id: "spawn",
              kind: "collaboration",
              payload: {
                operation: "spawnAgent",
                receiverLabels: ["Reviewer"],
                agentKeys: ["private-child"],
                agentStates: [["private-child", "completed"]],
              },
            },
            {
              ...identity,
              id: "native",
              kind: "collaboration",
              payload: {
                operation: "completed",
                receiverLabels: [],
                agentKey: "private-child",
                agentKeys: [],
                agentStates: [],
                label: "Subagent",
              },
            },
          ],
          activities: [],
        },
      ],
    } as unknown as AcpChatPage;
    render(
      <SubagentSurface page={page} itemId="native" agentKey="private-child" />,
    );
    expect(screen.getByRole("heading", { name: "Reviewer" })).toBeVisible();
    expect(screen.getByRole("status")).toHaveTextContent("Done");
    expect(readChild).not.toHaveBeenCalled();
  });
  it("uses the generic role for an explicitly selected unnamed child without reading", () => {
    readChild.mockClear();
    const page = {
      projectId: "project",
      chatId: "chat",
      currentSessionId: "session",
      sessions: [],
      turns: [
        {
          id: "turn",
          sessionId: "session",
          items: [
            {
              id: "spawn",
              kind: "collaboration",
              payload: {
                operation: "spawnAgent",
                receiverLabels: [""],
                agentKeys: ["private-child"],
                agentStates: [["private-child", "running"]],
              },
            },
          ],
          activities: [],
        },
      ],
    } as unknown as AcpChatPage;
    render(
      <SubagentSurface page={page} itemId="spawn" agentKey="private-child" />,
    );
    expect(screen.getByRole("heading", { name: "Subagent" })).toBeVisible();
    expect(readChild).not.toHaveBeenCalled();
  });
  it("shows an attributed current child through read-only chat rows", async () => {
    readChild.mockResolvedValue({
      kind: "current",
      snapshot: {
        childThreadId: "secret",
        parentThreadId: "parent",
        retentionLimited: true,
        turns: [
          {
            id: "native-turn",
            status: "completed",
            items: [
              {
                id: "answer",
                role: "assistant",
                lifecycle: "completed",
                tool_state: null,
                payload: { kind: "text", payload: { text: "Child answer" } },
              },
              {
                id: "compact",
                lifecycle: "completed",
                tool_state: null,
                payload: { kind: "compactionNotice", payload: {} },
              },
            ],
          },
        ],
      },
    });
    const page = {
      projectId: "project",
      chatId: "chat",
      projectionRevision: "7",
      currentSessionId: "session",
      sessions: [
        {
          id: "session",
          runtimeEpoch: "2",
          capabilities: { childTranscriptRead: true },
        },
      ],
      turns: [
        {
          id: "turn",
          sessionId: "session",
          runtimeEpoch: "2",
          items: [
            {
              id: "spawn",
              kind: "collaboration",
              payload: {
                agentKey: "secret",
                receiverLabels: ["Reviewer"],
                agentStates: [],
              },
            },
          ],
          activities: [],
        },
      ],
    } as unknown as AcpChatPage;
    render(
      <SubagentSurface
        page={page}
        projectGeneration="3"
        itemId="spawn"
        agentKey="secret"
      />,
    );
    expect(screen.getByText(/Loading child transcript/)).toBeVisible();
    expect(await screen.findByText("Child answer")).toBeVisible();
    expect(screen.getByText("Context automatically compacted")).toBeVisible();
    expect(screen.getByText("Current child transcript")).toBeVisible();
    expect(screen.getByText(/Some later child activity/)).toBeVisible();
    expect(readChild).toHaveBeenCalledWith("get_acp_child_transcript", {
      request: expect.objectContaining({
        itemId: "spawn",
        agentKey: "secret",
        expectedProjectionRevision: "7",
      }),
    });
  });

  it("shows retained tools and neutral legacy text from a stopped current Session", async () => {
    readChild.mockResolvedValue({
      kind: "fromEarlierSession",
      snapshot: {
        childThreadId: "secret",
        parentThreadId: "parent",
        retentionLimited: false,
        turns: [
          {
            id: "child-turn",
            status: "completed",
            items: [
              {
                id: "old-text",
                lifecycle: "completed",
                tool_state: null,
                payload: { kind: "text", payload: { text: "Legacy note" } },
              },
              {
                id: "reason",
                lifecycle: "completed",
                tool_state: null,
                payload: {
                  kind: "thoughtSummary",
                  payload: { summary: "Reviewing" },
                },
              },
              {
                id: "plan",
                lifecycle: "completed",
                tool_state: null,
                payload: {
                  kind: "plan",
                  payload: { summary: "Inspect child output" },
                },
              },
              {
                id: "cmd",
                lifecycle: "completed",
                tool_state: "completed",
                payload: {
                  kind: "toolCall",
                  payload: {
                    category: "execute",
                    label: "Check files",
                    inputSummary: "rg TODO",
                    outputSummary: "No matches",
                    locations: [],
                    execution: {
                      cwd: null,
                      exitCode: 1,
                      outputTruncated: false,
                    },
                  },
                },
              },
              {
                id: "file",
                lifecycle: "completed",
                tool_state: null,
                payload: {
                  kind: "fileChangeDiff",
                  payload: {
                    summary: "Updated file",
                    resource: {
                      kind: "unavailable",
                      reason: "missing",
                      label: "File location unavailable",
                    },
                    additions: 2,
                    deletions: 1,
                    operation: "updated",
                  },
                },
              },
            ],
          },
        ],
      },
    });
    const page = {
      projectId: "project",
      chatId: "chat",
      projectionRevision: "7",
      currentSessionId: "session",
      sessions: [
        {
          id: "session",
          runtimeEpoch: null,
          capabilities: { childTranscriptRead: true },
        },
      ],
      turns: [
        {
          id: "turn",
          sessionId: "session",
          runtimeEpoch: "2",
          items: [
            {
              id: "spawn",
              kind: "collaboration",
              payload: {
                agentKeys: ["secret"],
                receiverLabels: ["Reviewer"],
                agentStates: [],
              },
            },
          ],
          activities: [],
        },
      ],
    } as unknown as AcpChatPage;
    render(
      <SubagentSurface
        page={page}
        projectGeneration="3"
        itemId="spawn"
        agentKey="secret"
      />,
    );
    expect(await screen.findByText("From an earlier session")).toBeVisible();
    expect(screen.getByText("Legacy note")).toBeVisible();
    expect(screen.getByText("Unattributed")).toBeVisible();
    expect(screen.queryByText("Reviewing")).not.toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Agent plan" })).toHaveAttribute(
      "data-activity-kind",
      "plan",
    );
    expect(screen.getByText("Inspect child output")).toBeVisible();
    expect(screen.getByText("Check files")).toBeVisible();
    expect(screen.getByText("Updated file")).toBeVisible();
    expect(document.querySelector('[data-item-id="file"]')).toBeInTheDocument();
    expect(document.querySelector('[data-acp-item-id="file"]')).toBeNull();
  });

  it("discards a previous child's late read after selection changes", async () => {
    let complete!: (value: unknown) => void;
    readChild.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          complete = resolve;
        }),
    );
    readChild.mockResolvedValueOnce({ kind: "tooLarge" });
    const page = {
      projectId: "project",
      chatId: "chat",
      projectionRevision: "7",
      currentSessionId: "session",
      sessions: [
        {
          id: "session",
          runtimeEpoch: "2",
          capabilities: { childTranscriptRead: true },
        },
      ],
      turns: [
        {
          id: "turn",
          sessionId: "session",
          runtimeEpoch: "2",
          items: ["a", "b"].map((id) => ({
            id,
            kind: "collaboration",
            payload: {
              agentKeys: [id],
              receiverLabels: ["Reviewer"],
              agentStates: [],
            },
          })),
          activities: [],
        },
      ],
    } as unknown as AcpChatPage;
    const view = render(
      <SubagentSurface
        page={page}
        projectGeneration="3"
        itemId="a"
        agentKey="a"
      />,
    );
    view.rerender(
      <SubagentSurface
        page={page}
        projectGeneration="3"
        itemId="b"
        agentKey="b"
      />,
    );
    expect(await screen.findByText(/too large to display/)).toBeVisible();
    complete({
      kind: "current",
      snapshot: {
        childThreadId: "a",
        parentThreadId: "parent",
        retentionLimited: false,
        turns: [],
      },
    });
    await waitFor(() =>
      expect(screen.queryByText("Current child transcript")).toBeNull(),
    );
  });

  it("does not read a superseded Session", () => {
    readChild.mockClear();
    const page = {
      projectId: "project",
      chatId: "chat",
      currentSessionId: "new",
      sessions: [
        {
          id: "old",
          runtimeEpoch: null,
          capabilities: { childTranscriptRead: true },
        },
      ],
      turns: [
        {
          id: "turn",
          sessionId: "old",
          runtimeEpoch: "2",
          items: [
            {
              id: "spawn",
              kind: "collaboration",
              payload: {
                agentKeys: ["secret"],
                receiverLabels: ["Reviewer"],
                agentStates: [],
              },
            },
          ],
          activities: [],
        },
      ],
    } as unknown as AcpChatPage;
    render(
      <SubagentSurface
        page={page}
        projectGeneration="3"
        itemId="spawn"
        agentKey="secret"
      />,
    );
    expect(
      screen.getByText(/earlier session and isn’t available here/),
    ).toBeVisible();
    expect(readChild).not.toHaveBeenCalled();
  });
  it("keeps the rendered child list within the WebView row budget", async () => {
    readChild.mockResolvedValue({
      kind: "current",
      snapshot: {
        childThreadId: "secret",
        parentThreadId: "parent",
        retentionLimited: false,
        turns: [
          {
            id: "native-turn",
            status: "completed",
            items: Array.from({ length: 257 }, (_, index) => ({
              id: `native-${index}`,
              role: "assistant",
              lifecycle: "completed",
              tool_state: null,
              payload: { kind: "text", payload: { text: `Line ${index}` } },
            })),
          },
        ],
      },
    });
    const page = {
      projectId: "project",
      chatId: "chat",
      projectionRevision: "7",
      currentSessionId: "session",
      sessions: [
        {
          id: "session",
          runtimeEpoch: "2",
          capabilities: { childTranscriptRead: true },
        },
      ],
      turns: [
        {
          id: "turn",
          sessionId: "session",
          runtimeEpoch: "2",
          items: [
            {
              id: "spawn",
              kind: "collaboration",
              payload: {
                agentKey: "secret",
                receiverLabels: ["Reviewer"],
                agentStates: [],
              },
            },
          ],
          activities: [],
        },
      ],
    } as unknown as AcpChatPage;
    render(
      <SubagentSurface
        page={page}
        projectGeneration="3"
        itemId="spawn"
        agentKey="secret"
      />,
    );
    expect(
      await screen.findByText(/Showing the first 256 retained child items/),
    ).toBeVisible();
    expect(document.querySelectorAll("[data-item-id]")).toHaveLength(256);
    expect(screen.queryByText("Line 256")).toBeNull();
  });
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
              projectId: "project",
              chatId: "chat",
              sessionId: "session",
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

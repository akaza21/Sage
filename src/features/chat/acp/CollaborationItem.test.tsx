import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import type { AcpItem } from "../../../domain/acp";
import {
  agentArtifactFollowRecordFor,
  useAgentArtifactFollowStore,
} from "../../../app/agentArtifactFollowStore";
import { safeItem } from "../../../services/acpDesktopResponseItems";
import {
  subagentPaneForPage,
  type SubagentPaneTarget,
  useSubagentPaneStore,
} from "../../../app/subagentPaneStore";
import { CollaborationItem } from "./CollaborationItem";

afterEach(cleanup);

describe("CollaborationItem", () => {
  it.each(["running", "completed"])(
    "names a non-BMP %s child beside its shape glyph, never its initial",
    (state) => {
      const item = {
        kind: "collaboration",
        lifecycle: "completed",
        payload: {
          activityId: "activity",
          label: "spawnAgent",
          summary: "",
          receiverLabels: ["𐐀gent"],
          operation: "spawnAgent",
          agentKeys: ["child"],
          agentStates: [["child", state]],
        },
      } as unknown as Extract<AcpItem, { kind: "collaboration" }>;
      const { container } = render(<CollaborationItem item={item} />);
      // Codex s4/f0485: the glyph is the child's coloured shape.
      expect(
        container.querySelector(".acp-subagent-row__glyph"),
      ).toBeEmptyDOMElement();
      expect(container.querySelector(".acp-subagent-row")).toHaveTextContent(
        "𐐀gent",
      );
      expect(container.querySelector(".acp-subagent-row")).toHaveAttribute(
        "data-state",
        state === "running" ? "started working" : "finished",
      );
    },
  );
  it.each([
    { labels: ["", ""], participants: null },
    { labels: ["", "Bob"], participants: "Participants: Bob" },
    { labels: ["  ", "Bob", ""], participants: "Participants: Bob" },
  ])(
    "omits blank fallback participants from $labels",
    ({ labels, participants }) => {
      const item = {
        kind: "collaboration",
        lifecycle: "completed",
        payload: {
          activityId: null,
          label: "wait",
          summary: "Waiting for children",
          receiverLabels: labels,
          operation: "wait",
          agentKeys: ["unknown", "known"],
          agentStates: [],
        },
      } as unknown as Extract<AcpItem, { kind: "collaboration" }>;
      render(<CollaborationItem item={item} />);
      if (participants)
        expect(screen.getByText(participants)).toBeInTheDocument();
      else expect(screen.queryByText(/Participants:/)).toBeNull();
    },
  );

  it("opens an owner-scoped Subagent tab from a verified row", () => {
    useSubagentPaneStore.getState().clear();
    const item = safeItem({
      id: "spawn-1",
      projectId: "project-1",
      chatId: "chat-1",
      sessionId: "session-1",
      turnId: "turn-1",
      runtimeEpoch: "1",
      ordinal: 1,
      origin: "primary",
      activityId: null,
      lifecycle: "completed",
      revision: "1",
      createdAt: 1,
      updatedAt: 2,
      terminalAt: 2,
      kind: "collaboration",
      toolState: null,
      payload: {
        activityId: "activity-1",
        label: "spawnAgent",
        summary: "",
        receiverLabels: ["Reviewer"],
        operation: "spawnAgent",
        agentKeys: ["thread-secret"],
        agentStates: [["thread-secret", "running"]],
      },
    });
    if (item?.kind !== "collaboration") throw new Error("Rejected fixture");
    render(<CollaborationItem item={item} />);
    fireEvent.click(
      screen.getByRole("button", { name: "Open subagent Reviewer" }),
    );
    expect(useSubagentPaneStore.getState().open).toEqual({
      owner: {
        projectId: "project-1",
        chatId: "chat-1",
        sessionId: "session-1",
      },
      itemId: "spawn-1",
      agentKey: "thread-secret",
    });
  });
  it("shows a verified spawn as a compact working row without exposing thread IDs", () => {
    const item = {
      kind: "collaboration",
      lifecycle: "completed",
      payload: {
        activityId: "activity-1",
        label: "spawnAgent",
        summary: "",
        receiverLabels: [],
        operation: "spawnAgent",
        agentKey: null,
        agentKeys: ["secret-thread-id"],
        agentStates: [["secret-thread-id", "running"]],
      },
    } as unknown as Extract<AcpItem, { kind: "collaboration" }>;
    render(<CollaborationItem item={item} />);
    expect(
      screen.getByRole("status", { name: "Subagent started working" }),
    ).toHaveTextContent("Subagent started working");
    expect(screen.queryByText(/secret-thread-id/)).toBeNull();
  });

  it("does not claim a failed or unverified spawn started working", () => {
    for (const state of ["errored", "notFound", "shutdown", "interrupted"]) {
      const item = {
        kind: "collaboration",
        lifecycle: "completed",
        payload: {
          activityId: "activity-1",
          label: "spawnAgent",
          summary: "",
          receiverLabels: [],
          operation: "spawnAgent",
          agentKeys: ["thread-1"],
          agentStates: [["thread-1", state]],
        },
      } as unknown as Extract<AcpItem, { kind: "collaboration" }>;
      const { unmount } = render(<CollaborationItem item={item} />);
      expect(screen.queryByText(/started working/)).toBeNull();
      expect(screen.getByRole("status")).toHaveTextContent(
        /failed to start|stopped/,
      );
      if (state === "errored" || state === "notFound")
        expect(screen.queryByRole("button")).toBeNull();
      unmount();
    }
  });

  it("shows an interrupted child as stopped", () => {
    const item = {
      kind: "collaboration",
      lifecycle: "completed",
      payload: {
        activityId: "activity-1",
        label: "Code review",
        summary: "",
        receiverLabels: [],
        operation: "interrupted",
        agentKey: "thread-1",
        agentKeys: [],
        agentStates: [],
      },
    } as unknown as Extract<AcpItem, { kind: "collaboration" }>;
    render(<CollaborationItem item={item} />);
    expect(
      screen.getByRole("status", { name: "Code review stopped" }),
    ).toBeVisible();
  });

  it("shows an emitted completed child activity by its supplied name", () => {
    const item = {
      kind: "collaboration",
      lifecycle: "completed",
      payload: {
        activityId: null,
        label: "Reviewer",
        summary: "",
        receiverLabels: [],
        operation: "completed",
        agentKey: "child-1",
        agentKeys: [],
        agentStates: [],
      },
    } as unknown as Extract<AcpItem, { kind: "collaboration" }>;
    render(<CollaborationItem item={item} />);
    expect(
      screen.getByRole("status", { name: "Reviewer finished" }),
    ).toBeVisible();
    expect(screen.queryByText(/started working/)).toBeNull();
  });

  it("groups verified completions with available display names", () => {
    const item = safeItem({
      id: "wait-1",
      projectId: "project-1",
      chatId: "chat-1",
      sessionId: "session-1",
      turnId: "turn-1",
      runtimeEpoch: "1",
      ordinal: 1,
      origin: "primary",
      activityId: null,
      lifecycle: "completed",
      revision: "1",
      createdAt: 1,
      updatedAt: 2,
      terminalAt: 2,
      kind: "collaboration",
      toolState: null,
      payload: {
        activityId: "activity-2",
        label: "wait",
        summary: "",
        receiverLabels: ["A", "B", "C"],
        operation: "wait",
        agentKeys: ["id-a", "id-b", "id-c"],
        agentStates: [
          ["id-a", "completed"],
          ["id-b", "completed"],
          ["id-c", "completed"],
        ],
      },
    });
    if (item?.kind !== "collaboration")
      throw new Error("Projected completion was rejected.");
    render(<CollaborationItem item={item} />);
    expect(
      screen.queryByRole("button", { name: /A, B and C finished/ }),
    ).toBeNull();
    expect(
      screen.getByRole("status", { name: "A, B and C finished" }),
    ).toHaveTextContent("A, B and C finished");
    expect(
      screen.getAllByRole("button", { name: /^Open subagent [ABC]$/ }),
    ).toHaveLength(3);
    expect(document.querySelectorAll(".acp-subagent-row__glyph")).toHaveLength(
      3,
    );
    expect(screen.queryByText(/id-a/)).toBeNull();
  });

  it("does not route one completed child from a grouped event", () => {
    const item = {
      id: "group-1",
      kind: "collaboration",
      lifecycle: "completed",
      payload: {
        activityId: "activity-1",
        label: "wait",
        summary: "",
        receiverLabels: ["A", "B"],
        operation: "wait",
        agentKey: "id-a",
        agentKeys: ["id-a", "id-b"],
        agentStates: [
          ["id-b", "completed"],
          ["id-a", "running"],
        ],
      },
    } as unknown as Extract<AcpItem, { kind: "collaboration" }>;
    render(<CollaborationItem item={item} />);
    expect(screen.getByText("B finished")).toBeVisible();
    expect(screen.queryByRole("button", { name: /B finished/ })).toBeNull();
  });

  it("colours grouped finish dots for the agents that actually completed", () => {
    const item = {
      id: "group-colours",
      kind: "collaboration",
      lifecycle: "completed",
      payload: {
        activityId: "activity-1",
        label: "wait",
        summary: "",
        receiverLabels: ["A", "B", "C"],
        operation: "wait",
        agentKeys: ["id-a", "id-b", "id-c"],
        agentStates: [
          ["id-a", "running"],
          ["id-b", "completed"],
          ["id-c", "completed"],
        ],
      },
    } as unknown as Extract<AcpItem, { kind: "collaboration" }>;
    render(<CollaborationItem item={item} />);
    expect(
      screen.getByRole("status", { name: "B and C finished" }),
    ).toBeVisible();
    const dots = document.querySelectorAll(".acp-subagent-row__glyph");
    expect(dots).toHaveLength(2);
    expect(dots[0]).toHaveClass("acp-subagent-row__glyph--0");
    expect(dots[1]).toHaveClass("acp-subagent-row__glyph--1");
  });

  it("does not route conflicting singular child identities", () => {
    const item = {
      id: "conflict-1",
      kind: "collaboration",
      lifecycle: "completed",
      payload: {
        activityId: "activity-1",
        label: "Reviewer",
        summary: "",
        receiverLabels: [],
        operation: "started",
        agentKey: "child-a",
        agentKeys: ["child-b"],
        agentStates: [["child-b", "running"]],
      },
    } as unknown as Extract<AcpItem, { kind: "collaboration" }>;
    render(<CollaborationItem item={item} />);
    expect(screen.getByText("Reviewer started working")).toBeVisible();
    expect(
      screen.queryByRole("button", { name: /Reviewer started working/ }),
    ).toBeNull();
  });

  it("projects harness-owned collaboration without creating Sage controls", () => {
    const label =
      "Coordinate the independent accessibility review across every visible workbench surface";
    const item = {
      kind: "collaboration",
      lifecycle: "streaming",
      payload: {
        activityId: "activity-1",
        label,
        summary: "Harness activity completed.",
        receiverLabels: ["Reviewer", "Builder"],
      },
    } as unknown as Extract<AcpItem, { kind: "collaboration" }>;
    render(<CollaborationItem item={item} />);
    const collaboration = screen.getByRole("group", {
      name: `Harness collaboration: ${label}`,
      description: "Read-only activity owned and scheduled by the harness",
    });
    expect(collaboration).toHaveTextContent("Participants: Reviewer, Builder");
    expect(
      within(collaboration).getByText("Collaboration"),
    ).toBeInTheDocument();
    expect(within(collaboration).getByText(label)).toHaveAttribute(
      "title",
      label,
    );
    expect(screen.getByRole("status", { name: "Active" })).toHaveAttribute(
      "data-lifecycle",
      "streaming",
    );
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });

  it("gives each grouped name its own target and opens that specific child", () => {
    useSubagentPaneStore.getState().clear();
    const spawn = (id: string, name: string) =>
      safeItem({
        id: `spawn-${id}`,
        projectId: "project-1",
        chatId: "chat-1",
        sessionId: "session-1",
        turnId: "turn-1",
        runtimeEpoch: "1",
        ordinal: 1,
        origin: "primary",
        activityId: null,
        lifecycle: "completed",
        revision: "1",
        createdAt: 1,
        updatedAt: 2,
        terminalAt: 2,
        kind: "collaboration",
        toolState: null,
        payload: {
          activityId: "activity-1",
          label: "spawnAgent",
          summary: "",
          receiverLabels: [name],
          operation: "spawnAgent",
          agentKeys: [id],
          agentStates: [[id, "running"]],
        },
      }) as Extract<AcpItem, { kind: "collaboration" }>;
    const items = [
      spawn("math", "Count math"),
      spawn("readme", "Count readme"),
    ];
    const { container } = render(
      <CollaborationItem item={items[0]} items={items} />,
    );
    expect(
      screen.getByRole("status", {
        name: "Count math and Count readme started working",
      }),
    ).toHaveTextContent("Count math and Count readme started working");
    const buttons = screen.getAllByRole("button");
    expect(buttons.map((button) => button.getAttribute("aria-label"))).toEqual([
      "Open subagent Count math",
      "Open subagent Count readme",
    ]);
    expect(container.querySelector("details, summary")).toBeNull();
    expect(container.querySelector(".acp-live-shimmer")).toBeNull();
    expect(container.querySelector(".acp-subagent-row__done")).toBeNull();
    fireEvent.click(buttons[1]);
    expect(useSubagentPaneStore.getState()).toMatchObject({
      open: {
        owner: {
          projectId: "project-1",
          chatId: "chat-1",
          sessionId: "session-1",
        },
        itemId: "spawn-readme",
        agentKey: "readme",
      },
    });
    expect(
      agentArtifactFollowRecordFor(
        useAgentArtifactFollowStore.getState().records,
        useSubagentPaneStore.getState().open?.owner,
      ),
    ).toMatchObject({
      mode: "pinned",
      selected: { kind: "subagents", list: false },
    });
  });

  it("resolves a grouped event's key to that child's own spawn in the same session", () => {
    const owner = {
      projectId: "project-1",
      chatId: "chat-1",
      sessionId: "session-1",
    } as unknown as SubagentPaneTarget["owner"];
    const collaboration = (id: string, payload: object) => ({
      ...owner,
      id,
      kind: "collaboration",
      payload: { receiverLabels: [], agentStates: [], ...payload },
    });
    const page = {
      projectId: "project-1",
      chatId: "chat-1",
      turns: [
        {
          sessionId: "session-1",
          items: [
            collaboration("spawn-a", {
              operation: "spawnAgent",
              agentKeys: ["id-a"],
            }),
            collaboration("spawn-b", {
              operation: "spawnAgent",
              agentKeys: ["id-b"],
            }),
            collaboration("wait", {
              operation: "wait",
              agentKeys: ["id-a", "id-b"],
            }),
          ],
        },
      ],
    } as unknown as Parameters<typeof subagentPaneForPage>[1];
    const target = { owner, itemId: "wait", agentKey: "id-b" };
    expect(subagentPaneForPage(target, page)).toEqual({
      owner,
      itemId: "spawn-b",
      agentKey: "id-b",
    });
    expect(
      subagentPaneForPage({ ...target, agentKey: "id-unknown" }, page),
    ).toBeNull();
    expect(
      subagentPaneForPage(
        {
          ...target,
          owner: { ...owner, sessionId: "session-2" as typeof owner.sessionId },
        },
        page,
      ),
    ).toBeNull();
  });

  it("routes each named child of one grouped completion by its own key", () => {
    useSubagentPaneStore.getState().clear();
    const item = safeItem({
      id: "wait-2",
      projectId: "project-1",
      chatId: "chat-1",
      sessionId: "session-1",
      turnId: "turn-1",
      runtimeEpoch: "1",
      ordinal: 1,
      origin: "primary",
      activityId: null,
      lifecycle: "completed",
      revision: "1",
      createdAt: 1,
      updatedAt: 2,
      terminalAt: 2,
      kind: "collaboration",
      toolState: null,
      payload: {
        activityId: "activity-2",
        label: "wait",
        summary: "",
        receiverLabels: ["A", "B", "C", "D", "E"],
        operation: "wait",
        agentKeys: ["id-a", "id-b", "id-c", "id-d", "id-e"],
        agentStates: ["a", "b", "c", "d", "e"].map((key) => [
          `id-${key}`,
          "completed",
        ]),
      },
    });
    if (item?.kind !== "collaboration") throw new Error("Rejected fixture");
    render(<CollaborationItem item={item} />);
    expect(
      screen.getByRole("status", { name: "A, B and 3 more finished" }),
    ).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Open subagent B" }));
    expect(useSubagentPaneStore.getState().open?.agentKey).toBe("id-b");
    fireEvent.click(
      screen.getByRole("button", { name: "Show 3 more subagents" }),
    );
    expect(
      agentArtifactFollowRecordFor(
        useAgentArtifactFollowStore.getState().records,
        useSubagentPaneStore.getState().open?.owner,
      ),
    ).toMatchObject({
      mode: "pinned",
      selected: { kind: "subagents", list: true },
    });
  });
});

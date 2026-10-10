import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import type { AcpItem } from "../../../domain/acp";
import { useSubagentPaneStore } from "../../../app/subagentPaneStore";
import { CollaborationItem } from "./CollaborationItem";
import { PrimaryActivities } from "./PrimaryActivities";

type Collaboration = Extract<AcpItem, { kind: "collaboration" }>;

function child(id: string, name: string, state = "running"): Collaboration {
  return {
    id,
    projectId: "project",
    chatId: "chat",
    sessionId: "session",
    turnId: "turn",
    runtimeEpoch: "1",
    origin: "primary",
    activityId: null,
    lifecycle: "completed",
    revision: "1",
    ordinal: 1,
    createdAt: 1,
    updatedAt: 2,
    terminalAt: 2,
    kind: "collaboration",
    toolState: null,
    payload: {
      label: "spawnAgent",
      summary: "",
      activityId: null,
      operation: "spawnAgent",
      agentKeys: [id],
      receiverLabels: [name],
      agentStates: [[id, state]],
    },
  } as unknown as Collaboration;
}

function activities(items: AcpItem[]) {
  return render(
    <PrimaryActivities
      items={items}
      interactive
      renderItem={(item) =>
        item.kind === "collaboration" ? (
          <CollaborationItem key={item.id} item={item} />
        ) : (
          <p key={item.id}>Interleaved activity</p>
        )
      }
    />,
  );
}

afterEach(cleanup);

describe("Subagent activity rows", () => {
  it("groups unnamed native start activities as a count rather than fabricated names", () => {
    const first = child("a", "");
    const second = child("b", "");
    for (const item of [first, second]) {
      item.payload.operation = "started";
      item.payload.agentKey = item.payload.agentKeys?.[0];
      item.payload.agentKeys = [];
      item.payload.agentStates = [];
      item.payload.label = "Subagent";
    }
    activities([first, second]);
    expect(screen.getByText("2 subagents started working")).toBeVisible();
    expect(
      screen.queryByText("Subagent and Subagent started working"),
    ).toBeNull();
  });

  it("merges compatible spawn and start observations and keeps later supplied name enrichment selectable", () => {
    const spawn = child("a", "");
    const started = {
      ...child("a", ""),
      id: "named-start" as Collaboration["id"],
    };
    started.payload.operation = "started";
    started.payload.agentKey = "a";
    started.payload.agentKeys = [];
    started.payload.agentStates = [];
    started.payload.label = "Reviewer";
    activities([spawn, started]);
    expect(screen.getAllByRole("status")).toHaveLength(1);
    fireEvent.click(
      screen.getByRole("button", {
        name: "Reviewer started working. Open Subagent",
      }),
    );
    expect(useSubagentPaneStore.getState().open).toEqual({
      owner: { projectId: "project", chatId: "chat", sessionId: "session" },
      itemId: "a",
      agentKey: "a",
    });
  });
  it("renders no subagent row for an empty activity list", () => {
    activities([]);
    expect(screen.queryByRole("status")).toBeNull();
    expect(screen.queryByRole("button")).toBeNull();
  });

  it("deduplicates repeated states inside an event and ignores undeclared child keys", () => {
    const item = child("a", "A", "completed");
    item.payload.agentStates = [
      ["a", "completed"],
      ["a", "completed"],
      ["foreign", "completed"],
    ];
    render(<CollaborationItem item={item} />);
    expect(screen.getByText("A finished")).toBeVisible();
    expect(document.querySelectorAll(".acp-subagent-row__glyph")).toHaveLength(
      1,
    );
    expect(screen.queryByText(/foreign|3 subagents/)).toBeNull();
  });

  it("keeps a 300-agent named summary bounded to emitted leading names and an exact remainder", () => {
    activities(
      Array.from({ length: 300 }, (_, index) =>
        child(`id-${index}`, `Reviewer ${index}`),
      ),
    );
    expect(
      screen.getByText("Reviewer 0, Reviewer 1 and 298 more started working"),
    ).toBeVisible();
    expect(screen.getAllByRole("status")).toHaveLength(1);
    expect(document.querySelectorAll(".acp-subagent-row__glyph")).toHaveLength(
      3,
    );
  });
  it("shows named stopped and failed children without claiming a later failure was a failed spawn", () => {
    const failed = child("a", "Reviewer", "errored");
    failed.payload.operation = "wait";
    activities([failed, child("b", "Builder", "interrupted")]);
    expect(screen.getByText("Reviewer failed")).toBeVisible();
    expect(screen.getByText("Builder stopped")).toBeVisible();
    expect(screen.queryByText(/failed to start/)).toBeNull();
    expect(screen.queryByRole("button", { name: /Reviewer/ })).toBeNull();
  });

  it("does not shimmer a finished row when the same event also contains a running child", () => {
    const item = child("a", "A");
    item.payload.agentKeys = ["a", "b"];
    item.payload.receiverLabels = ["A", "B"];
    item.payload.agentStates = [
      ["a", "running"],
      ["b", "completed"],
    ];
    render(<CollaborationItem item={item} />);
    expect(screen.getByText("B finished")).not.toHaveClass("acp-live-shimmer");
    expect(screen.getByText("A started working")).toHaveClass(
      "acp-live-shimmer",
    );
  });
  it("shows both named started and finished children in a mixed spawn event", () => {
    const item = child("a", "A");
    item.payload.agentKeys = ["a", "b"];
    item.payload.receiverLabels = ["A", "B"];
    item.payload.agentStates = [
      ["a", "running"],
      ["b", "completed"],
    ];
    render(<CollaborationItem item={item} />);
    expect(screen.getByText("A started working")).toBeVisible();
    expect(screen.getByText("B finished")).toBeVisible();
    expect(screen.queryByRole("button")).toBeNull();
  });

  it("groups adjacent starts and completions separately without merging across prose", () => {
    activities([
      child("a", "A"),
      child("b", "B"),
      child("c", "C", "completed"),
      child("d", "D", "completed"),
      { ...child("prose", ""), kind: "text" } as unknown as AcpItem,
      child("e", "E", "completed"),
    ]);
    expect(screen.getByText("A and B started working")).toBeVisible();
    expect(screen.getByText("C and D finished")).toBeVisible();
    expect(screen.getByText("E finished")).toBeVisible();
    expect(screen.getAllByRole("status")).toHaveLength(3);
    expect(screen.getAllByRole("button")).toHaveLength(1);
  });

  it("retains known names beside unnamed children without exposing identity keys", () => {
    activities([child("secret-a", "Reviewer"), child("secret-b", "")]);
    expect(
      screen.getByText("Reviewer and 1 more started working"),
    ).toBeVisible();
    expect(screen.queryByText(/secret-/)).toBeNull();
  });

  it("does not count duplicate child identities twice in adjacent completions", () => {
    activities([
      child("a", "A", "completed"),
      {
        ...child("a", "A", "completed"),
        id: "duplicate-event" as Collaboration["id"],
      },
    ]);
    expect(screen.getByText("A finished")).toBeVisible();
    expect(screen.getAllByRole("status")).toHaveLength(1);
  });

  it("bounds a 301-child run into a 300-child row and a remainder", () => {
    activities(
      Array.from({ length: 301 }, (_, index) => child(`child-${index}`, "")),
    );
    expect(screen.getByText("300 subagents started working")).toBeVisible();
    expect(screen.getByText("Subagent started working")).toBeVisible();
    expect(document.querySelectorAll(".acp-subagent-row__glyph")).toHaveLength(
      4,
    );
  });

  it("keeps session and Chat owners out of each other's adjacent groups", () => {
    for (const change of [
      { chatId: "other-chat" },
      { sessionId: "other-session" },
      { runtimeEpoch: "2" },
      { turnId: "other-turn" },
    ]) {
      const view = activities([
        child("a", "A"),
        { ...child("b", "B"), ...change } as Collaboration,
      ]);
      expect(screen.getByText("A started working")).toBeVisible();
      expect(screen.getByText("B started working")).toBeVisible();
      expect(screen.getAllByRole("status")).toHaveLength(2);
      view.unmount();
    }
  });
});

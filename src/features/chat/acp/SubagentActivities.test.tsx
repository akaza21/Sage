import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { AcpItem } from "../../../domain/acp";
import { useSubagentPaneStore } from "../../../app/subagentPaneStore";
import { CollaborationItem } from "./CollaborationItem";
import { PrimaryActivities } from "./PrimaryActivities";
import * as subagentActivity from "./subagentActivity";

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

const row = (name: string) => screen.getByRole("status", { name });
const queryRow = (name: string) => screen.queryByRole("status", { name });

afterEach(() => {
  cleanup();
  useSubagentPaneStore.getState().clear();
});

describe("Subagent activity rows", () => {
  it("refreshes an already-rendered unnamed group when later lifecycle observations emit names", () => {
    const spawn = [child("a", ""), child("b", "")];
    const view = activities(spawn);
    expect(row("2 subagents started working")).toBeVisible();
    const completed = spawn.map((member, index) => ({
      ...member,
      id: `done-${index}`,
      payload: {
        ...member.payload,
        operation: "completed",
        agentKey: index ? "b" : "a",
        label: index ? "Builder" : "Reviewer",
        agentKeys: [],
        receiverLabels: [],
        agentStates: [],
      },
    })) as unknown as Collaboration[];
    view.rerender(
      <PrimaryActivities
        items={[...spawn, ...completed]}
        interactive
        renderItem={(item) =>
          item.kind === "collaboration" ? (
            <CollaborationItem key={item.id} item={item} />
          ) : null
        }
      />,
    );
    expect(row("Reviewer and Builder started working")).toBeVisible();
  });

  it("deduplicates spawn and native completion and opens the owner-bearing activity", () => {
    const spawn = child("child-1", "Reviewer", "completed");
    spawn.id = "spawn" as Collaboration["id"];
    const completion = {
      ...spawn,
      id: "activity" as Collaboration["id"],
      payload: {
        ...spawn.payload,
        label: "Subagent",
        operation: "completed",
        agentKey: "child-1",
        agentKeys: [],
        receiverLabels: [],
        agentStates: [],
      },
    };
    activities([spawn, completion]);
    expect(screen.getAllByRole("status")).toHaveLength(1);
    expect(queryRow("2 subagents finished")).toBeNull();
    fireEvent.click(
      screen.getByRole("button", { name: "Open subagent Reviewer" }),
    );
    expect(useSubagentPaneStore.getState().open).toEqual({
      owner: { projectId: "project", chatId: "chat", sessionId: "session" },
      itemId: "activity",
      agentKey: "child-1",
    });
  });

  it("does not rebuild 300 unchanged members on streaming ticks but updates revised membership", () => {
    const summary = vi.spyOn(subagentActivity, "subagentSegments");
    const members = Array.from({ length: 300 }, (_, index) =>
      child(`child-${index}`, `Reviewer ${index}`),
    );
    const renderItem = (item: AcpItem) =>
      item.kind === "text" ? <p key={item.id}>{item.payload.text}</p> : null;
    const stream = (text: string) =>
      ({
        ...members[0],
        id: "reply",
        kind: "text",
        payload: { role: "assistant", text },
      }) as unknown as AcpItem;
    const content = (items: Collaboration[], text: string) => (
      <PrimaryActivities
        items={[...items, stream(text)]}
        interactive
        renderItem={renderItem}
      />
    );
    const view = render(content(members, "First tick"));
    expect(summary).toHaveBeenCalledOnce();
    summary.mockClear();
    const unchanged = structuredClone(members);
    view.rerender(content(unchanged, "Next tick"));
    expect(screen.getByText("Next tick")).toBeVisible();
    expect(summary).not.toHaveBeenCalled();

    const revised = structuredClone(unchanged);
    revised[1].revision = "2" as Collaboration["revision"];
    revised[1].payload.receiverLabels = ["Builder"];
    view.rerender(content(revised, "Next tick"));
    expect(
      row("Reviewer 0, Builder and 298 more started working"),
    ).toBeVisible();
    expect(summary).toHaveBeenCalledOnce();
    summary.mockClear();

    revised[1] = {
      ...child("replacement", "Replacement"),
      revision: "2" as Collaboration["revision"],
    };
    view.rerender(content(revised, "Next tick"));
    expect(
      row("Reviewer 0, Replacement and 298 more started working"),
    ).toBeVisible();
    expect(summary).toHaveBeenCalledOnce();
    summary.mockClear();

    view.rerender(content(revised.slice(0, -1), "Next tick"));
    expect(
      row("Reviewer 0, Replacement and 297 more started working"),
    ).toBeVisible();
    expect(summary).toHaveBeenCalledOnce();
  });

  it("keeps both adjacent verified children individually openable from their group", () => {
    const { container } = activities([
      child("a", "Reviewer"),
      child("b", "Builder"),
    ]);
    expect(row("Reviewer and Builder started working")).toHaveTextContent(
      "Reviewer and Builder started working",
    );
    expect(container.querySelector("details, summary")).toBeNull();
    for (const [name, key] of [
      ["Reviewer", "a"],
      ["Builder", "b"],
    ]) {
      const button = screen.getByRole("button", {
        name: `Open subagent ${name}`,
      });
      expect(button).toHaveTextContent(name);
      button.focus();
      expect(button).toHaveFocus();
      fireEvent.click(button);
      expect(useSubagentPaneStore.getState().open).toEqual({
        owner: { projectId: "project", chatId: "chat", sessionId: "session" },
        itemId: key,
        agentKey: key,
      });
    }
  });

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
    expect(row("2 subagents started working")).toBeVisible();
    expect(queryRow("Subagent and Subagent started working")).toBeNull();
  });

  it("opens each of three adjacent completed children using its own event and child key", () => {
    const items = [
      child("a", "Reviewer", "completed"),
      child("b", "Builder", "completed"),
      child("c", "Tester", "completed"),
    ].map((item) => ({
      ...item,
      id: `event-${item.id}` as Collaboration["id"],
    }));
    activities(items);
    expect(row("Reviewer, Builder and Tester finished")).toBeVisible();
    expect(
      screen.getAllByRole("button", { name: /^Open subagent / }),
    ).toHaveLength(3);
    for (const [name, key] of [
      ["Reviewer", "a"],
      ["Builder", "b"],
      ["Tester", "c"],
    ]) {
      fireEvent.click(
        screen.getByRole("button", { name: `Open subagent ${name}` }),
      );
      expect(useSubagentPaneStore.getState().open).toEqual({
        owner: { projectId: "project", chatId: "chat", sessionId: "session" },
        itemId: `event-${key}`,
        agentKey: key,
      });
    }
  });

  it("leaves an unverified child non-clickable beside a verified grouped child", () => {
    const unverified = child("b", "Builder");
    unverified.payload.agentKey = "mismatched-key";
    activities([child("a", "Reviewer"), unverified]);
    expect(row("Reviewer and Builder started working")).toBeVisible();
    expect(screen.queryByRole("button", { name: /Builder/ })).toBeNull();
    expect(screen.getAllByRole("button")).toHaveLength(1);
    fireEvent.click(
      screen.getByRole("button", { name: "Open subagent Reviewer" }),
    );
    expect(useSubagentPaneStore.getState().open).toEqual({
      owner: { projectId: "project", chatId: "chat", sessionId: "session" },
      itemId: "a",
      agentKey: "a",
    });
    expect(screen.queryByText(/mismatched-key/)).toBeNull();
  });

  it("names two leading children and sends the rest to the Subagents list", () => {
    activities(
      Array.from({ length: 300 }, (_, index) =>
        child(`id-${index}`, `Reviewer ${index}`),
      ),
    );
    expect(
      row("Reviewer 0, Reviewer 1 and 298 more started working"),
    ).toBeVisible();
    expect(
      screen
        .getAllByRole("button")
        .map((button) => button.getAttribute("aria-label")),
    ).toEqual([
      "Open subagent Reviewer 0",
      "Open subagent Reviewer 1",
      "Show 298 more subagents",
    ]);
    fireEvent.click(
      screen.getByRole("button", { name: "Open subagent Reviewer 1" }),
    );
    expect(useSubagentPaneStore.getState()).toMatchObject({
      open: { itemId: "id-1", agentKey: "id-1" },
      list: false,
    });
    fireEvent.click(
      screen.getByRole("button", { name: "Show 298 more subagents" }),
    );
    expect(useSubagentPaneStore.getState()).toMatchObject({
      open: { agentKey: "id-0" },
      selected: true,
      list: true,
    });
    expect(screen.getAllByRole("status")).toHaveLength(1);
    expect(document.querySelectorAll(".acp-subagent-row__glyph")).toHaveLength(
      3,
    );
  });

  it("never fabricates per-name targets for an unnamed group", () => {
    const ambiguous = child("a", "");
    ambiguous.payload.agentKeys = ["a", "b"];
    ambiguous.payload.receiverLabels = ["", ""];
    ambiguous.payload.agentStates = [
      ["a", "running"],
      ["b", "running"],
    ];
    const verified = {
      ...child("a", ""),
      id: "verified-event" as Collaboration["id"],
    };
    activities([ambiguous, verified]);
    expect(row("2 subagents started working")).toHaveTextContent(
      "2 subagents started working",
    );
    expect(screen.queryByRole("button")).toBeNull();
  });

  it("merges compatible spawn and start observations and selects the native source with its supplied name", () => {
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
      screen.getByRole("button", { name: "Open subagent Reviewer" }),
    );
    expect(useSubagentPaneStore.getState().open).toEqual({
      owner: { projectId: "project", chatId: "chat", sessionId: "session" },
      itemId: "named-start",
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
    expect(row("A finished")).toBeVisible();
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
      row("Reviewer 0, Reviewer 1 and 298 more started working"),
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
    expect(row("Reviewer failed")).toBeVisible();
    expect(row("Builder stopped")).toBeVisible();
    expect(screen.queryByText(/failed to start/)).toBeNull();
    expect(screen.queryByRole("button", { name: /Reviewer/ })).toBeNull();
  });

  it("does not shimmer live or finished subagent rows", () => {
    const item = child("a", "A");
    item.payload.agentKeys = ["a", "b"];
    item.payload.receiverLabels = ["A", "B"];
    item.payload.agentStates = [
      ["a", "running"],
      ["b", "completed"],
    ];
    const { container } = render(<CollaborationItem item={item} />);
    expect(row("B finished")).toBeVisible();
    expect(row("A started working")).toBeVisible();
    expect(container.querySelector(".acp-live-shimmer")).toBeNull();
    expect(
      container.querySelector('[data-state="started working"]'),
    ).toContainElement(row("A started working"));
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
    expect(row("A started working")).toBeVisible();
    expect(row("B finished")).toBeVisible();
    // One grouped event: each declared key routes to its own spawn.
    expect(
      screen
        .getAllByRole("button")
        .map((button) => button.getAttribute("aria-label")),
    ).toEqual(["Open subagent A", "Open subagent B"]);
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
    expect(row("A and B started working")).toBeVisible();
    expect(row("C and D finished")).toBeVisible();
    expect(row("E finished")).toBeVisible();
    expect(screen.getAllByRole("status")).toHaveLength(3);
    expect(screen.getAllByRole("button")).toHaveLength(5);
  });

  it("retains known names beside unnamed children without exposing identity keys", () => {
    activities([child("secret-a", "Reviewer"), child("secret-b", "")]);
    expect(row("Reviewer and 1 more started working")).toBeVisible();
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
    expect(row("A finished")).toBeVisible();
    expect(screen.getAllByRole("status")).toHaveLength(1);
  });

  it("bounds a 301-child run into a 300-child row and a remainder", () => {
    activities(
      Array.from({ length: 301 }, (_, index) => child(`child-${index}`, "")),
    );
    expect(row("300 subagents started working")).toBeVisible();
    expect(row("Subagent started working")).toBeVisible();
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
      expect(row("A started working")).toBeVisible();
      expect(row("B started working")).toBeVisible();
      expect(screen.getAllByRole("status")).toHaveLength(2);
      view.unmount();
    }
  });
});

describe("subagentSegments", () => {
  const states = [
    "started working",
    "finished",
    "failed to start",
    "failed",
    "stopped",
  ];
  const render = (labels: string[], state: string) =>
    subagentActivity
      .subagentSegments(labels, state)
      .map((segment) =>
        segment.index !== undefined
          ? `[${segment.index}:${segment.text}]`
          : segment.more
            ? `{${segment.text}}`
            : segment.text,
      )
      .join("");

  it.each(states)("joins 1, 2, 3 and 5 names for %s", (state) => {
    expect(render(["A"], state)).toBe(`[0:A] ${state}`);
    expect(render(["A", "B"], state)).toBe(`[0:A] and [1:B] ${state}`);
    expect(render(["A", "B", "C"], state)).toBe(
      `[0:A], [1:B] and [2:C] ${state}`,
    );
    expect(render(["A", "B", "C", "D", "E"], state)).toBe(
      `[0:A], [1:B] and {3 more} ${state}`,
    );
    expect(
      subagentActivity.subagentSummary(["A", "B", "C", "D", "E"], state),
    ).toBe(`A, B and 3 more ${state}`);
  });

  it("keeps each name's own index past unnamed children and never names them", () => {
    expect(render(["", "B"], "finished")).toBe("[1:B] and {1 more} finished");
    expect(render([""], "finished")).toBe("[0:Subagent] finished");
    expect(render(["", ""], "finished")).toBe("2 subagents finished");
  });
});

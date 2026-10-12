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

afterEach(() => {
  cleanup();
  useSubagentPaneStore.getState().clear();
});

describe("Subagent activity rows", () => {
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
    expect(screen.queryByText("2 subagents finished")).toBeNull();
    fireEvent.click(
      screen.getByRole("button", { name: "Reviewer finished. Open Subagent" }),
    );
    expect(useSubagentPaneStore.getState().open).toEqual({
      owner: { projectId: "project", chatId: "chat", sessionId: "session" },
      itemId: "activity",
      agentKey: "child-1",
    });
  });

  it("does not rebuild 300 unchanged members on streaming ticks but updates revised membership", () => {
    const summary = vi.spyOn(subagentActivity, "subagentSummary");
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
      screen.getByText("Reviewer 0, Builder and 298 more started working"),
    ).toBeVisible();
    expect(summary).toHaveBeenCalledOnce();
    summary.mockClear();

    revised[1] = {
      ...child("replacement", "Replacement"),
      revision: "2" as Collaboration["revision"],
    };
    view.rerender(content(revised, "Next tick"));
    expect(
      screen.getByText("Reviewer 0, Replacement and 298 more started working"),
    ).toBeVisible();
    expect(summary).toHaveBeenCalledOnce();
    summary.mockClear();

    view.rerender(content(revised.slice(0, -1), "Next tick"));
    expect(
      screen.getByText("Reviewer 0, Replacement and 297 more started working"),
    ).toBeVisible();
    expect(summary).toHaveBeenCalledOnce();
  });

  it("keeps both adjacent verified children individually openable from their group", () => {
    activities([child("a", "Reviewer"), child("b", "Builder")]);
    const summary = screen
      .getByText("Reviewer and Builder started working")
      .closest("summary");
    expect(summary).not.toBeNull();
    expect(summary).toHaveAccessibleName(
      "Reviewer and Builder started working. Show Subagents",
    );
    expect(screen.queryByRole("button", { name: /Open Subagent/ })).toBeNull();
    const group = summary!.parentElement as HTMLDetailsElement;
    group.open = true;
    fireEvent(group, new Event("toggle"));
    for (const [name, key] of [
      ["Reviewer", "a"],
      ["Builder", "b"],
    ]) {
      const button = screen.getByRole("button", {
        name: `${name} started working. Open Subagent`,
      });
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
    expect(screen.getByText("2 subagents started working")).toBeVisible();
    expect(
      screen.queryByText("Subagent and Subagent started working"),
    ).toBeNull();
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
    const summary = screen
      .getByText("Reviewer, Builder and Tester finished")
      .closest("summary");
    expect(summary).not.toBeNull();
    const group = summary!.parentElement as HTMLDetailsElement;
    group.open = true;
    fireEvent(group, new Event("toggle"));
    expect(
      screen.getAllByRole("button", { name: /Open Subagent/ }),
    ).toHaveLength(3);
    for (const [name, key] of [
      ["Reviewer", "a"],
      ["Builder", "b"],
      ["Tester", "c"],
    ]) {
      fireEvent.click(
        screen.getByRole("button", { name: `${name} finished. Open Subagent` }),
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
    const summary = screen
      .getByText("Reviewer and Builder started working")
      .closest("summary");
    expect(summary).not.toBeNull();
    const group = summary!.parentElement as HTMLDetailsElement;
    group.open = true;
    fireEvent(group, new Event("toggle"));
    expect(screen.getByText("Builder started working")).toBeVisible();
    expect(screen.queryByRole("button", { name: /Builder/ })).toBeNull();
    expect(
      screen.getAllByRole("button", { name: /Open Subagent/ }),
    ).toHaveLength(1);
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
    expect(screen.queryByText(/mismatched-key/)).toBeNull();
  });

  it("keeps all 300 children individually reachable and unmounts their actions when collapsed", () => {
    activities(
      Array.from({ length: 300 }, (_, index) =>
        child(`id-${index}`, `Reviewer ${index}`),
      ),
    );
    const summary = screen
      .getByText("Reviewer 0, Reviewer 1 and 298 more started working")
      .closest("summary");
    expect(summary).not.toBeNull();
    expect(screen.queryByRole("button", { name: /Open Subagent/ })).toBeNull();
    const group = summary!.parentElement as HTMLDetailsElement;
    group.open = true;
    fireEvent(group, new Event("toggle"));
    const buttons = screen.getAllByRole("button", { name: /Open Subagent/ });
    expect(buttons).toHaveLength(300);
    for (const [index, button] of buttons.entries()) {
      expect(button).toHaveAccessibleName(
        `Reviewer ${index} started working. Open Subagent`,
      );
      fireEvent.click(button);
      expect(useSubagentPaneStore.getState().open).toEqual({
        owner: { projectId: "project", chatId: "chat", sessionId: "session" },
        itemId: `id-${index}`,
        agentKey: `id-${index}`,
      });
    }
    group.open = false;
    fireEvent(group, new Event("toggle"));
    expect(screen.queryByRole("button", { name: /Open Subagent/ })).toBeNull();
    expect(screen.getAllByRole("status")).toHaveLength(1);
    expect(document.querySelectorAll(".acp-subagent-row__glyph")).toHaveLength(
      3,
    );
  });

  it("admits a later verified unnamed observation without making other grouped children clickable", () => {
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
    const summary = screen
      .getByText("2 subagents started working")
      .closest("summary");
    expect(summary).not.toBeNull();
    const group = summary!.parentElement as HTMLDetailsElement;
    group.open = true;
    fireEvent(group, new Event("toggle"));
    const buttons = screen.getAllByRole("button", {
      name: "Subagent started working. Open Subagent",
    });
    expect(buttons).toHaveLength(1);
    fireEvent.click(buttons[0]);
    expect(useSubagentPaneStore.getState().open).toEqual({
      owner: { projectId: "project", chatId: "chat", sessionId: "session" },
      itemId: "verified-event",
      agentKey: "a",
    });
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
      screen.getByRole("button", {
        name: "Reviewer started working. Open Subagent",
      }),
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

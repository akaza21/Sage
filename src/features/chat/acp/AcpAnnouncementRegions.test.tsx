import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { AcpChatPage, AcpItem, TurnProjection } from "../../../domain/acp";
import { AcpAnnouncementRegions } from "./AcpAnnouncementRegions";
import { AcpChatProjection } from "./AcpChatProjection";
import { TurnHeader } from "./TurnHeader";
import { StrictMode } from "react";
import { snapshotPage } from "../../../app/agentDiff.testFixture";
import * as editingFiles from "./editingFiles";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

function page(
  revision: string,
  state: TurnProjection["state"],
  items: readonly AcpItem[] = [],
  chatId = "chat-a",
): AcpChatPage {
  return {
    projectId: "project-a",
    chatId,
    title: "Durable chat",
    projectionRevision: revision,
    currentSessionId: null,
    sessions: [],
    turns: [
      {
        id: "turn-a",
        sessionId: "session-a",
        runtimeEpoch: "1",
        ordinal: 1,
        state,
        revision,
        createdAt: 1,
        updatedAt: 2,
        terminalAt: state === "completed" ? 2 : null,
        items,
        activities: [],
      },
    ],
    nextCursor: null,
  } as unknown as AcpChatPage;
}

function tool(
  state: "inProgress" | "completed",
  id = "tool-a",
  label = "Read source",
): AcpItem {
  return {
    id,
    kind: "toolCall",
    lifecycle: state === "completed" ? "completed" : "streaming",
    toolState: state,
    ordinal: 1,
    payload: {
      presentation: { kind: "other", rawKind: null, label, summary: null },
      inputSummary: null,
    },
  } as unknown as AcpItem;
}

function permission(resolved: boolean): AcpItem {
  return {
    id: "permission-a",
    kind: "permissionRequest",
    lifecycle: "completed",
    toolState: "declined",
    ordinal: 2,
    payload: {
      requestId: "permission-a",
      state: resolved ? "resolvedDeclined" : "declinePending",
      label: "Write file",
      summary: "Denied",
    },
  } as unknown as AcpItem;
}

function regions(container: HTMLElement) {
  return {
    polite: container.querySelector<HTMLElement>(
      '[data-acp-announcement="polite"]',
    )!,
    assertive: container.querySelector<HTMLElement>(
      '[data-acp-announcement="assertive"]',
    )!,
  };
}

function observeNames(region: HTMLElement) {
  const names: string[] = [];
  const observer = new MutationObserver(() => {
    const name = region.getAttribute("aria-label");
    if (name) names.push(name);
  });
  observer.observe(region, {
    attributes: true,
    attributeFilter: ["aria-label"],
  });
  return { names, stop: () => observer.disconnect() };
}

describe("AcpAnnouncementRegions", () => {
  it("scans edits only for changed turn objects through accepted streaming pages", () => {
    const initial = snapshotPage();
    const retained = initial.turns[0];
    const active = {
      ...retained,
      id: "active" as never,
      ordinal: 2,
      state: "running" as const,
      items: [],
    };
    initial.turns = [retained, active];
    const scan = vi.spyOn(editingFiles, "isEditingFiles");
    const { rerender } = render(<AcpAnnouncementRegions page={initial} />);
    scan.mockClear();
    rerender(
      <AcpAnnouncementRegions
        page={{ ...initial, projectionRevision: "2" as never }}
      />,
    );
    expect(scan).not.toHaveBeenCalled();
    rerender(
      <AcpAnnouncementRegions
        page={{
          ...initial,
          projectionRevision: "3" as never,
          turns: [retained, { ...active, revision: "2" as never }],
        }}
      />,
    );
    expect(scan.mock.calls.map(([, turn]) => turn.id)).toEqual([
      "active",
      "active",
    ]);
  });
  it.each([false, true])(
    "resets mid-edit session changes without announcing a finish (new edit=%s)",
    async (newEdit) => {
      const initial = snapshotPage();
      initial.turns[0].state = "running";
      initial.turns[0].items = initial.turns[0].items.map(
        (item) => ({ ...item, lifecycle: "streaming" }) as AcpItem,
      );
      const { container, rerender } = render(
        <AcpAnnouncementRegions page={initial} />,
      );
      const region = regions(container).polite;
      const observation = observeNames(region);
      await waitFor(() => expect(region).toHaveAccessibleName("Editing files"));
      const changed = {
        ...initial,
        projectionRevision: "2" as never,
        currentSessionId: "new-session" as never,
        sessions: [
          ...initial.sessions,
          { ...initial.sessions[0], id: "new-session" as never },
        ],
      };
      if (newEdit)
        changed.turns = [
          ...initial.turns,
          {
            ...initial.turns[0],
            id: "new-turn" as never,
            ordinal: 2,
            sessionId: changed.currentSessionId,
            items: initial.turns[0].items.map(
              (item) =>
                ({
                  ...item,
                  turnId: "new-turn" as never,
                  sessionId: changed.currentSessionId,
                }) as AcpItem,
            ),
          },
        ];
      rerender(<AcpAnnouncementRegions page={changed} />);
      await act(
        () =>
          new Promise<void>((resolve) =>
            requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
          ),
      );
      if (newEdit) expect(region).toHaveAccessibleName("Editing files");
      else expect(region).toBeEmptyDOMElement();
      expect(observation.names).toEqual(
        newEdit ? ["Editing files", "Editing files"] : ["Editing files"],
      );
      observation.stop();
    },
  );
  it.each([true, false])(
    "on focus announces only current edits and the latest terminal turn (editing=%s)",
    async (editing) => {
      let frameId = 0;
      const frames = new Map<number, FrameRequestCallback>();
      vi.stubGlobal(
        "requestAnimationFrame",
        (callback: FrameRequestCallback) => {
          frames.set(++frameId, callback);
          return frameId;
        },
      );
      vi.stubGlobal("cancelAnimationFrame", (frame: number) =>
        frames.delete(frame),
      );
      const initial = snapshotPage();
      initial.turns[0].state = "running";
      const saved = initial.turns[0].items[0];
      initial.turns[0].items = [];
      const { container, rerender } = render(
        <AcpAnnouncementRegions page={initial} enabled={false} />,
      );
      const region = regions(container).polite;
      const observation = observeNames(region);
      for (let revision = 2; revision <= 1102; revision += 1) {
        const background = structuredClone(initial);
        background.projectionRevision = String(revision) as never;
        background.turns[0].items = [
          tool("completed", `tool-${revision}`, `Stale tool ${revision}`),
          {
            ...saved,
            lifecycle: revision % 2 ? "completed" : "streaming",
          } as AcpItem,
        ];
        rerender(<AcpAnnouncementRegions page={background} enabled={false} />);
      }
      const current = structuredClone(initial);
      current.projectionRevision = "1103" as never;
      current.turns[0].items = [
        { ...saved, lifecycle: editing ? "streaming" : "completed" } as AcpItem,
      ];
      current.turns = [
        {
          ...initial.turns[0],
          id: "older" as never,
          ordinal: 0,
          state: "completed",
        },
        current.turns[0],
      ];
      if (!editing) current.turns[1].state = "failed";
      rerender(<AcpAnnouncementRegions page={current} enabled={false} />);
      expect(region).toBeEmptyDOMElement();
      rerender(<AcpAnnouncementRegions page={current} enabled />);
      for (let drained = 0; frames.size > 0 && drained < 2205; drained += 1) {
        const callbacks = [...frames.values()];
        frames.clear();
        await act(async () => {
          callbacks.forEach((callback) => callback(0));
        });
      }
      expect(frames.size).toBe(0);
      expect(region).toHaveAccessibleName(editing ? "Completed" : "Failed");
      expect(regions(container).assertive).toBeEmptyDOMElement();
      expect(observation.names).toEqual([
        editing ? "Editing files" : "Finished editing files",
        editing ? "Completed" : "Failed",
      ]);
      observation.stop();
    },
  );
  it("keeps its region mounted and catches up only the current edit state on focus", async () => {
    const editing = snapshotPage();
    editing.turns[0].state = "running";
    const saved = editing.turns[0].items[0];
    editing.turns[0].items = [{ ...saved, lifecycle: "streaming" } as AcpItem];
    const { container, rerender } = render(
      <AcpChatProjection page={editing} active focused />,
    );
    const region = regions(container).polite;
    const observation = observeNames(region);
    await waitFor(() => expect(region).toHaveTextContent("Editing files"));
    rerender(<AcpChatProjection page={editing} active focused={false} />);
    expect(region).toBeInTheDocument();
    expect(region).toHaveAttribute("aria-live", "off");
    expect(region).toBeEmptyDOMElement();
    rerender(<AcpChatProjection page={editing} active focused />);
    expect(regions(container).polite).toBe(region);
    expect(region).toBeEmptyDOMElement();
    await waitFor(() => expect(region).toHaveAccessibleName("Editing files"));
    const finished = structuredClone(editing);
    finished.projectionRevision = "2" as never;
    finished.turns[0].items = [saved];
    rerender(<AcpChatProjection page={finished} active focused={false} />);
    expect(region).toBeEmptyDOMElement();
    rerender(<AcpChatProjection page={finished} active focused />);
    await waitFor(() =>
      expect(region).toHaveTextContent("Finished editing files"),
    );
    expect(observation.names).toEqual([
      "Editing files",
      "Editing files",
      "Finished editing files",
    ]);
    observation.stop();
  });
  it("starts a fresh edit announcement when the mounted region changes chat scope", async () => {
    const initial = snapshotPage();
    const editing = structuredClone(initial);
    editing.chatId = "chat-b" as never;
    editing.turns[0].state = "running";
    editing.turns[0].items = editing.turns[0].items.map(
      (item) =>
        ({
          ...item,
          chatId: editing.chatId,
          lifecycle: "streaming",
        }) as AcpItem,
    );
    const { container, rerender } = render(
      <AcpAnnouncementRegions page={initial} />,
    );
    const region = regions(container).polite;
    const observation = observeNames(region);
    rerender(<AcpAnnouncementRegions page={editing} />);
    await waitFor(() => expect(region).toHaveTextContent("Editing files"));
    rerender(<AcpAnnouncementRegions page={{ ...editing }} />);
    expect(regions(container).polite).toBe(region);
    expect(observation.names).toEqual(["Editing files"]);
    observation.stop();
  });
  it.each(["completed", "failed", "cancelled"] as const)(
    "announces initially active edits and their %s finish without replaying history",
    async (lifecycle) => {
      const editing = snapshotPage();
      editing.turns[0].state = "running";
      const saved = editing.turns[0].items[0];
      editing.turns[0].items = [
        { ...saved, lifecycle: "streaming" } as AcpItem,
      ];
      const { container, rerender } = render(
        <AcpAnnouncementRegions page={editing} />,
        { wrapper: StrictMode },
      );
      const region = regions(container).polite;
      expect(region).toBeEmptyDOMElement();
      const observation = observeNames(region);
      await waitFor(() => expect(region).toHaveTextContent("Editing files"));
      const finished = structuredClone(editing);
      finished.projectionRevision = "2" as never;
      finished.turns[0].items = [{ ...saved, lifecycle } as AcpItem];
      rerender(<AcpAnnouncementRegions page={finished} />);
      await waitFor(() =>
        expect(region).toHaveTextContent("Finished editing files"),
      );
      rerender(<AcpAnnouncementRegions page={editing} />);
      expect(observation.names).toEqual([
        "Editing files",
        "Finished editing files",
      ]);
      observation.stop();
    },
  );
  it("announces edit start and finish once through the persistent empty region", async () => {
    const initial = snapshotPage();
    initial.turns[0].state = "running";
    const saved = initial.turns[0].items[0];
    initial.turns[0].items = [];
    const { container, rerender } = render(
      <AcpAnnouncementRegions page={initial} />,
    );
    const region = regions(container).polite;
    expect(region).toBeEmptyDOMElement();
    const observation = observeNames(region);
    const editing = structuredClone(initial);
    editing.projectionRevision = "2" as never;
    editing.turns[0].items = [{ ...saved, lifecycle: "streaming" } as AcpItem];
    rerender(<AcpAnnouncementRegions page={editing} />);
    await waitFor(() => expect(region).toHaveTextContent("Editing files"));
    rerender(
      <AcpAnnouncementRegions
        page={{ ...editing, projectionRevision: "3" as never }}
      />,
    );
    const finished = structuredClone(editing);
    finished.projectionRevision = "4" as never;
    finished.turns[0].items = [saved];
    rerender(<AcpAnnouncementRegions page={finished} />);
    await waitFor(() =>
      expect(region).toHaveTextContent("Finished editing files"),
    );
    rerender(
      <AcpAnnouncementRegions
        page={{ ...finished, projectionRevision: "5" as never }}
      />,
    );
    expect(regions(container).polite).toBe(region);
    expect(observation.names).toEqual([
      "Editing files",
      "Finished editing files",
    ]);
    observation.stop();
  });
  it.each([
    ["interrupted", "cancelledByUser", "Cancelled by you"],
    ["cancelled", "cancelledByUser", "Cancelled by you"],
    ["interrupted", undefined, "Interrupted"],
    ["cancelled", undefined, "Cancelled"],
    ["completed", undefined, "Completed"],
    ["failed", "timeout", "Failed"],
  ] as const)(
    "announces the rendered header label once for %s with reason %s",
    async (state, kind, label) => {
      const initial = page("1", "running");
      const settled = page("2", state);
      settled.turns[0].terminalReason = kind ? { kind } : undefined;
      const { container, rerender } = render(
        <>
          <TurnHeader turn={initial.turns[0]} />
          <AcpAnnouncementRegions page={initial} />
        </>,
      );
      const observation = observeNames(regions(container).polite);
      const content = (
        <>
          <TurnHeader turn={settled.turns[0]} />
          <AcpAnnouncementRegions page={settled} />
        </>
      );
      rerender(content);
      expect(container.querySelector("[data-turn-status]")).toHaveTextContent(
        label,
      );
      expect(
        container.querySelector("[data-turn-status]"),
      ).toHaveAccessibleName(label);
      await waitFor(() =>
        expect(regions(container).polite).toHaveAccessibleName(label),
      );
      expect(regions(container).polite).toHaveTextContent(label);
      rerender(
        <>
          <TurnHeader turn={settled.turns[0]} />
          <AcpAnnouncementRegions page={{ ...settled }} />
        </>,
      );
      expect(observation.names).toEqual([label]);
      observation.stop();
    },
  );

  it("keeps the first page and an identical rerender silent", () => {
    const first = page("1", "completed", [tool("completed"), permission(true)]);
    const { container, rerender } = render(
      <AcpAnnouncementRegions page={first} />,
    );
    expect(regions(container).polite).toBeEmptyDOMElement();
    expect(regions(container).assertive).toBeEmptyDOMElement();

    rerender(<AcpAnnouncementRegions page={{ ...first }} />);
    expect(regions(container).polite).toBeEmptyDOMElement();
    expect(regions(container).assertive).toBeEmptyDOMElement();
  });

  it("announces a later terminal Turn exactly once", async () => {
    const { container, rerender } = render(
      <AcpAnnouncementRegions page={page("1", "running")} />,
    );
    const observation = observeNames(regions(container).polite);
    rerender(<AcpAnnouncementRegions page={page("2", "completed")} />);
    await waitFor(() =>
      expect(regions(container).polite).toHaveAccessibleName("Completed"),
    );
    expect(
      screen.getByRole("status", { name: "Completed" }),
    ).toBeInTheDocument();
    expect(regions(container).assertive).toBeEmptyDOMElement();

    rerender(<AcpAnnouncementRegions page={page("2", "completed")} />);
    expect(regions(container).polite).toHaveAccessibleName("Completed");
    expect(observation.names).toEqual(["Completed"]);
    observation.stop();
  });

  it("uses the exact polite tool and assertive permission terminal names", async () => {
    const { container, rerender } = render(
      <AcpAnnouncementRegions
        page={page("1", "running", [tool("inProgress"), permission(false)])}
      />,
    );
    rerender(
      <AcpAnnouncementRegions
        page={page("2", "running", [tool("completed"), permission(true)])}
      />,
    );
    await waitFor(() =>
      expect(regions(container).polite).toHaveAccessibleName(
        "Read source: Completed",
      ),
    );
    expect(
      screen.getByRole("status", { name: "Read source: Completed" }),
    ).toBeInTheDocument();
    expect(regions(container).assertive).toHaveAccessibleName(
      "Permission declined",
    );
    expect(
      screen.getByRole("status", { name: "Permission declined" }),
    ).toBeInTheDocument();
  });

  it("drains coalesced Turn and tool terminal messages individually", async () => {
    const initial = page("1", "running", [
      tool("inProgress"),
      tool("inProgress", "tool-b", "Run checks"),
    ]);
    const { container, rerender } = render(
      <AcpAnnouncementRegions page={initial} />,
    );
    const observation = observeNames(regions(container).polite);
    rerender(
      <AcpAnnouncementRegions
        page={page("2", "completed", [
          tool("completed"),
          tool("completed", "tool-b", "Run checks"),
        ])}
      />,
    );
    await waitFor(() =>
      expect(observation.names).toEqual([
        "Completed",
        "Read source: Completed",
        "Run checks: Completed",
      ]),
    );
    observation.stop();
  });

  it("preserves a coalesced drain across a newer projection", async () => {
    const initial = page("1", "running", [
      tool("inProgress"),
      tool("inProgress", "tool-b", "Run checks"),
    ]);
    const terminalItems = [
      tool("completed"),
      tool("completed", "tool-b", "Run checks"),
    ];
    const { container, rerender } = render(
      <AcpAnnouncementRegions page={initial} />,
    );
    const observation = observeNames(regions(container).polite);
    rerender(
      <AcpAnnouncementRegions page={page("2", "completed", terminalItems)} />,
    );
    rerender(
      <AcpAnnouncementRegions page={page("3", "completed", terminalItems)} />,
    );
    await waitFor(() =>
      expect(observation.names).toEqual([
        "Completed",
        "Read source: Completed",
        "Run checks: Completed",
      ]),
    );
    observation.stop();
  });

  it("announces a coalesced new latest terminal Turn", async () => {
    const current = page("1", "running");
    const { rerender } = render(<AcpAnnouncementRegions page={current} />);
    const next = page("2", "completed");
    const value = {
      ...next,
      turns: [current.turns[0], { ...next.turns[0], id: "turn-b", ordinal: 2 }],
    } as unknown as AcpChatPage;
    rerender(<AcpAnnouncementRegions page={value} />);
    await waitFor(() =>
      expect(
        screen.getByRole("status", { name: "Completed" }),
      ).toBeInTheDocument(),
    );
  });

  it("silences history merges and scope changes", () => {
    const current = page("3", "completed");
    const { container, rerender } = render(
      <AcpAnnouncementRegions page={current} />,
    );
    const earlier = page("4", "completed", [tool("completed")]);
    const history = {
      ...earlier,
      turns: [
        { ...earlier.turns[0], id: "turn-earlier", ordinal: 0 },
        current.turns[0],
      ],
    } as unknown as AcpChatPage;
    rerender(<AcpAnnouncementRegions page={history} />);
    expect(regions(container).polite).toBeEmptyDOMElement();

    rerender(
      <AcpAnnouncementRegions
        page={page("4", "completed", [tool("completed")], "chat-b")}
      />,
    );
    expect(regions(container).polite).toBeEmptyDOMElement();
  });

  it("cancels a pending drain before a scope change can announce it", async () => {
    const initial = page("1", "running", [tool("inProgress")]);
    const { container, rerender } = render(
      <AcpAnnouncementRegions page={initial} />,
    );
    const observation = observeNames(regions(container).polite);
    rerender(
      <AcpAnnouncementRegions
        page={page("2", "completed", [tool("completed")])}
      />,
    );
    rerender(
      <AcpAnnouncementRegions page={page("1", "running", [], "chat-b")} />,
    );
    await act(
      () =>
        new Promise<void>((resolve) => {
          requestAnimationFrame(() => requestAnimationFrame(() => resolve()));
        }),
    );
    expect(observation.names).toEqual([]);
    expect(regions(container).polite).toBeEmptyDOMElement();
    observation.stop();
  });
});

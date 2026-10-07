import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import type { AcpChatPage, AcpItem, TurnProjection } from "../../../domain/acp";
import { AcpAnnouncementRegions } from "./AcpAnnouncementRegions";

afterEach(cleanup);

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

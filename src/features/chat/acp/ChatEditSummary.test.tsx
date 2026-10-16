import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import type { AcpItem, TurnProjection } from "../../../domain/acp";
import { snapshotPage } from "../../../app/agentDiff.testFixture";
import { acpDesktop } from "../../../services/acpDesktop";
import { AcpChatPresentation } from "./AcpChatPresentation";
import * as fileChip from "./chatFileChip";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

it.each([
  [7, 4, "+11-5", "+7", "+4"],
  [800000, 800000, "+1600000-5", "+999+", "+999+"],
] as const)(
  "sums latest-revision row totals (%s, %s) in the pill and opens those same diffs",
  (firstAdditions, secondAdditions, totalLabel, firstLabel, secondLabel) => {
    const page = snapshotPage();
    const original = page.turns[0];
    const saved = original.items[0];
    if (saved.kind !== "fileChangeDiff") throw new Error("Expected edit");
    const latest = {
      ...original,
      id: "latest" as never,
      ordinal: 2,
      items: [
        {
          ...saved,
          turnId: "latest" as never,
          payload: {
            ...saved.payload,
            changes: [
              {
                ...saved.payload.changes[0],
                additions: firstAdditions,
                deletions: 2,
              },
            ],
          },
        },
        {
          ...saved,
          id: "second" as never,
          turnId: "latest" as never,
          ordinal: 2,
          payload: {
            ...saved.payload,
            changes: [
              {
                ...saved.payload.changes[0],
                additions: secondAdditions,
                deletions: 3,
                resource: {
                  kind: "resource" as const,
                  resource: { rootId: "root-a", relativePath: "src/b.ts" },
                },
              },
            ],
          },
        },
      ],
    };
    page.turns = [original, latest];
    const open = vi.fn();
    render(
      <AcpChatPresentation
        page={page}
        hasEarlier={false}
        loadingEarlier={false}
        historyError={null}
        projectRoots={[]}
        retentionIncomplete={false}
        editSummaryContainer={document.body}
        onOpenResource={open}
      />,
    );
    const pill = screen.getByRole("button", { name: /2 files changed/ });
    expect(within(pill).getByLabelText("Line changes")).toHaveTextContent(
      totalLabel,
    );
    fireEvent.click(pill);
    for (const [name, additions, deletions, itemId] of [
      ["a.ts", firstLabel, 2, saved.id],
      ["b.ts", secondLabel, 3, "second"],
    ] as const) {
      const row = screen.getByText(name).closest("li")!;
      expect(row).toHaveTextContent(additions);
      expect(row).toHaveTextContent(`-${deletions}`);
      fireEvent.click(within(row).getByRole("button", { name: /Open diff/ }));
      expect(open).toHaveBeenLastCalledWith(
        expect.anything(),
        "diff",
        expect.anything(),
        expect.objectContaining({ turnId: "latest", itemId }),
      );
    }
  },
);

it("keeps the real expanded summary asleep on unrelated presentation renders", () => {
  const props = {
    page: snapshotPage(),
    hasEarlier: false,
    loadingEarlier: false,
    historyError: null,
    projectRoots: [],
    retentionIncomplete: false,
    onOpenResource: vi.fn(),
    editSummaryContainer: document.body,
  };
  const { rerender } = render(<AcpChatPresentation {...props} />);
  fireEvent.click(screen.getByRole("button", { name: /1 file changed/ }));
  const names = vi.spyOn(fileChip, "chatFileChipBasename");
  rerender(
    <AcpChatPresentation
      {...props}
      projectRoots={[]}
      historyError="History refresh failed"
    />,
  );
  expect(names).not.toHaveBeenCalled();
  expect(screen.getByText("a.ts")).toBeVisible();
  rerender(<AcpChatPresentation {...props} page={{ ...props.page }} />);
  expect(names).toHaveBeenCalled();
});

it.each([
  [null, null],
  [null, 0],
  [0, null],
] as const)(
  "renders unknown totals independently without placeholders (%s, %s)",
  (additions, deletions) => {
    const page = snapshotPage();
    const edit = page.turns[0].items[0];
    if (edit.kind !== "fileChangeDiff") throw new Error("Expected edit");
    edit.payload = {
      ...edit.payload,
      changes: [{ ...edit.payload.changes[0], additions, deletions }],
    };
    render(
      <AcpChatPresentation
        page={page}
        hasEarlier={false}
        loadingEarlier={false}
        historyError={null}
        projectRoots={[]}
        retentionIncomplete={false}
        editSummaryContainer={document.body}
      />,
    );
    const header = screen.getByRole("button", { name: /1 file changed/ });
    const totals = within(header).queryByLabelText("Line changes");
    if (additions === null && deletions === null) expect(totals).toBeNull();
    else {
      expect(totals).toHaveTextContent(additions === null ? "-0" : "+0");
      expect(totals?.children).toHaveLength(1);
    }
  },
);

it("keeps prior-session rows visible but entirely noninteractive", () => {
  const page = snapshotPage();
  const current = page.turns[0];
  const saved = current.items[0];
  if (saved.kind !== "fileChangeDiff") throw new Error("Expected saved edit");
  page.sessions = [
    ...page.sessions,
    { ...page.sessions[0], id: "previous" as never },
  ];
  page.turns = [
    {
      ...current,
      id: "archived" as never,
      sessionId: "previous" as never,
      items: [
        {
          ...saved,
          turnId: "archived" as never,
          sessionId: "previous" as never,
          payload: {
            ...saved.payload,
            changes: [
              {
                ...saved.payload.changes[0],
                resource: {
                  kind: "resource",
                  resource: {
                    rootId: "root-a",
                    relativePath: "src/archived.ts",
                  },
                },
              },
            ],
          },
        },
      ],
    },
    current,
  ];
  render(
    <AcpChatPresentation
      page={page}
      hasEarlier={false}
      loadingEarlier={false}
      historyError={null}
      projectRoots={[]}
      retentionIncomplete={false}
      editSummaryContainer={document.body}
      onOpenResource={vi.fn()}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: /2 files changed/ }));
  const archived = screen.getByText("archived.ts").closest("li")!;
  expect(within(archived).queryAllByRole("button")).toHaveLength(0);
  const active = screen.getByText("a.ts").closest("li")!;
  expect(
    within(active).getByRole("button", { name: /Open diff/ }),
  ).toBeEnabled();
});

it("renders no file summary until its portal container exists", () => {
  const props = {
    page: snapshotPage(),
    hasEarlier: false,
    loadingEarlier: false,
    historyError: null,
    projectRoots: [],
    retentionIncomplete: false,
  };
  const { container, rerender } = render(<AcpChatPresentation {...props} />);
  expect(
    screen.queryByRole("region", { name: "Files edited in this chat" }),
  ).toBeNull();
  const anchor = document.createElement("div");
  container.append(anchor);
  rerender(<AcpChatPresentation {...props} editSummaryContainer={anchor} />);
  expect(
    within(anchor).getByRole("region", { name: "Files edited in this chat" }),
  ).toBeVisible();
  rerender(<AcpChatPresentation {...props} />);
  expect(anchor).toBeEmptyDOMElement();
});

it("opens the originating saved Diff and records only the chosen retained Turn review", async () => {
  const page = snapshotPage();
  const original = page.turns[0];
  const saved = original.items[0];
  if (saved.kind !== "fileChangeDiff")
    throw new Error("Expected a saved edit fixture");
  page.turns = [
    original,
    {
      ...original,
      id: "turn-b" as TurnProjection["id"],
      ordinal: 2,
      items: [
        {
          ...saved,
          id: "item-b" as AcpItem["id"],
          turnId: "turn-b" as AcpItem["turnId"],
          payload: {
            ...saved.payload,
            changes: [
              {
                ...saved.payload.changes[0],
                resource: {
                  kind: "resource",
                  resource: { rootId: "root-a", relativePath: "src/b.ts" },
                },
              },
            ],
          },
        } as AcpItem,
      ],
    } as TurnProjection,
  ];
  const open = vi.fn();
  const record = vi
    .spyOn(acpDesktop, "recordAgentEditDisposition")
    .mockResolvedValue({} as never);
  render(
    <AcpChatPresentation
      page={page}
      editSummaryContainer={document.body}
      onOpenResource={open}
      hasEarlier={false}
      loadingEarlier={false}
      historyError={null}
      projectRoots={[{ id: "root-a", name: "Root", ordinal: 0 }]}
      retentionIncomplete={false}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: /2 files changed/ }));
  fireEvent.click(
    screen.getByRole("button", { name: "Open diff Root / src/a.ts" }),
  );
  expect(open).toHaveBeenCalledWith(
    { rootId: "root-a", relativePath: "src/a.ts" },
    "diff",
    { projectId: "project-a", chatId: "chat-a", sessionId: "session-a" },
    expect.objectContaining({ turnId: "turn-a", itemId: "item-a" }),
  );
  fireEvent.click(
    within(screen.getByLabelText("Review turn 1")).getByRole("button", {
      name: "Mark reviewed",
    }),
  );
  await waitFor(() =>
    expect(record).toHaveBeenCalledExactlyOnceWith({
      projectId: "project-a",
      chatId: "chat-a",
      sessionId: "session-a",
      turnId: "turn-a",
      expectedRevision: "1",
      state: "accepted",
    }),
  );
});

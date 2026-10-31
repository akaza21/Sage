import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import {
  snapshotPage,
  snapshotRef,
  snapshotResource,
} from "../../../app/agentDiff.fixture";
import type { AcpItem } from "../../../domain/acp";
import { AcpChatPresentation } from "./AcpChatPresentation";
import { TurnFileActivity } from "./TurnFileActivity";

afterEach(cleanup);

it("names a single edited file by basename and opens its exact saved diff", () => {
  const page = snapshotPage();
  const opened: unknown[] = [];
  render(
    <TurnFileActivity
      page={page}
      turn={page.turns[0]}
      onOpenResource={(...request) => opened.push(request)}
    />,
  );
  expect(screen.getByText("Edited a.ts")).toHaveAttribute("title", "a.ts");
  fireEvent.click(screen.getByRole("button", { name: "View changes" }));
  expect(opened).toEqual([
    [
      snapshotResource,
      "diff",
      {
        projectId: page.projectId,
        chatId: page.chatId,
        sessionId: page.turns[0].sessionId,
      },
      snapshotRef,
    ],
  ]);
});

it("qualifies same-path files across roots and preserves each saved diff identity", () => {
  const page = snapshotPage();
  const edit = page.turns[0].items[0];
  if (edit.kind !== "fileChangeDiff") throw new Error("Expected edit");
  page.turns[0].items = ["root-a", "root-b"].map((rootId, ordinal) => ({
    ...edit,
    id: `edit-${rootId}` as typeof edit.id,
    ordinal,
    payload: {
      ...edit.payload,
      changes: [
        {
          ...edit.payload.changes[0],
          resource: {
            kind: "resource" as const,
            resource: { rootId, relativePath: "src/index.ts" },
          },
        },
      ],
    },
  }));
  const onOpenResource = vi.fn();
  const props = { page, turn: page.turns[0], onOpenResource };
  const { rerender } = render(
    <TurnFileActivity
      {...props}
      rootLabels={{ "root-a": "App (root 1)", "root-b": "App (root 2)" }}
    />,
  );
  for (const [ordinal, rootId] of ["root-a", "root-b"].entries()) {
    const label = `App (root ${ordinal + 1}) / src/index.ts`;
    const button = screen.getByRole("button", {
      name: new RegExp(label.replace(/[()]/g, "\\$&")),
    });
    expect(button).toHaveAttribute("title", label);
    fireEvent.click(button);
    expect(onOpenResource).toHaveBeenLastCalledWith(
      { rootId, relativePath: "src/index.ts" },
      "diff",
      expect.objectContaining({ chatId: page.chatId }),
      expect.objectContaining({ rootId, relativePath: "src/index.ts" }),
    );
  }
  rerender(<TurnFileActivity {...props} rootLabels={{}} />);
  expect(
    screen.getByRole("button", { name: /root-b \/ src\/index.ts/ }),
  ).toHaveAttribute("title", "root-b / src/index.ts");
});

it("refreshes root labels in memoized turns without changing their saved diff identity", async () => {
  const page = snapshotPage();
  const onOpenResource = vi.fn();
  const props = {
    page,
    onOpenResource,
    hasEarlier: false,
    loadingEarlier: false,
    historyError: null,
    retentionIncomplete: false,
  };
  const { rerender } = render(
    <AcpChatPresentation
      {...props}
      projectRoots={[{ id: "root-a", name: "App", ordinal: 0 }]}
    />,
  );
  expect(
    await screen.findByRole("button", { name: /App \/ src\/a.ts/ }),
  ).toBeVisible();
  rerender(
    <AcpChatPresentation
      {...props}
      projectRoots={[{ id: "root-a", name: "Renamed", ordinal: 0 }]}
    />,
  );
  fireEvent.click(
    await screen.findByRole("button", { name: /Renamed \/ src\/a.ts/ }),
  );
  expect(screen.queryByRole("button", { name: /App \/ src\/a.ts/ })).toBeNull();
  expect(onOpenResource).toHaveBeenCalledWith(
    snapshotResource,
    "diff",
    expect.anything(),
    snapshotRef,
  );
});

it("shows real line counts and opens the originating saved Diff from the edit card", () => {
  const page = snapshotPage();
  const onOpenResource = vi.fn();
  render(
    <TurnFileActivity
      page={page}
      turn={page.turns[0]}
      onOpenResource={onOpenResource}
    />,
  );
  const card = screen.getByRole("region", { name: "Edited files" });
  expect(card).toHaveTextContent("Edited a.ts");
  expect(within(card).getAllByLabelText("Line changes")[0]).toHaveTextContent(
    "+1-1",
  );
  expect(within(card).queryByRole("button", { name: "Undo" })).toBeNull();
  fireEvent.click(within(card).getByRole("button", { name: "View changes" }));
  expect(onOpenResource).toHaveBeenLastCalledWith(
    snapshotResource,
    "diff",
    {
      projectId: page.projectId,
      chatId: page.chatId,
      sessionId: page.currentSessionId,
    },
    snapshotRef,
  );
  fireEvent.click(within(card).getByRole("button", { name: /src\/a.ts/ }));
  expect(onOpenResource).toHaveBeenCalledTimes(2);
});

it("does not offer a working diff or invent line counts when a saved pair is unavailable", () => {
  const page = snapshotPage();
  const edit = page.turns[0].items[0];
  if (edit.kind !== "fileChangeDiff") throw new Error("Expected edit");
  edit.payload.changes = [
    {
      ...edit.payload.changes[0],
      pairState: "unavailable",
      additions: null,
      deletions: null,
      snapshotRevision: "0",
      oldBytes: null,
      newBytes: null,
      contentDigest: null,
    },
  ];
  render(
    <TurnFileActivity
      page={page}
      turn={page.turns[0]}
      onOpenResource={vi.fn()}
    />,
  );
  const card = screen.getByRole("region", { name: "Edited files" });
  expect(within(card).queryByLabelText("Line changes")).toBeNull();
  expect(card).not.toHaveTextContent("—");
  expect(
    within(card).getByRole("button", { name: "View changes" }),
  ).toBeDisabled();
  expect(
    within(card).getByRole("button", { name: /src\/a.ts/ }),
  ).toBeDisabled();
});

it("keeps the edit card bounded to three rows until requested and 32 retained files overall", () => {
  const page = snapshotPage();
  const saved = page.turns[0].items[0];
  if (saved.kind !== "fileChangeDiff") throw new Error("Expected edit");
  page.turns[0].items = Array.from({ length: 34 }, (_, index) => ({
    ...saved,
    id: `bounded-edit-${index}` as never,
    ordinal: index,
    payload: {
      ...saved.payload,
      changes: [
        {
          ...saved.payload.changes[0],
          resource: {
            kind: "resource",
            resource: {
              rootId: "root-a",
              relativePath: `src/bounded-${index}.ts`,
            },
          },
        },
      ],
    },
  }));
  render(<TurnFileActivity page={page} turn={page.turns[0]} />);
  const card = screen.getByRole("region", { name: "Edited files" });
  expect(within(card).getAllByRole("listitem")).toHaveLength(3);
  fireEvent.click(
    within(card).getByRole("button", { name: "Show 29 more files" }),
  );
  expect(within(card).getAllByRole("listitem")).toHaveLength(32);
  expect(card).toHaveTextContent("2 additional changed files not shown.");
  fireEvent.click(
    within(card).getByRole("button", { name: "Show fewer files" }),
  );
  expect(within(card).getAllByRole("listitem")).toHaveLength(3);
});

it("does not enable actions for an earlier session's edits", () => {
  const page = snapshotPage();
  page.currentSessionId = "other-session" as never;
  const onOpenResource = vi.fn();
  render(
    <TurnFileActivity
      page={page}
      turn={page.turns[0]}
      onOpenResource={onOpenResource}
    />,
  );
  const card = screen.getByRole("region", { name: "Edited files" });
  expect(
    within(card)
      .getAllByRole("button")
      .every((button) => (button as HTMLButtonElement).disabled),
  ).toBe(true);
  expect(onOpenResource).not.toHaveBeenCalled();
});

it("counts all 33 known edited files without presenting retained-subset line totals", async () => {
  const page = snapshotPage();
  const saved = page.turns[0].items[0];
  if (saved.kind !== "fileChangeDiff") throw new Error("Expected edit");
  page.turns[0].items = Array.from({ length: 33 }, (_, index) => ({
    ...saved,
    id: `edit-${index}`,
    ordinal: index,
    payload: {
      truncated: false,
      changes: [
        {
          resource: {
            kind: "resource",
            resource: {
              rootId: "root-a",
              relativePath: `src/file-${index}.ts`,
            },
          },
          originalResource: null,
          operation: "updated",
          summary: "Reported change",
          additions: 1,
          deletions: 1,
        },
      ],
    },
  })) as unknown as AcpItem[];
  const { container } = render(
    <AcpChatPresentation
      page={page}
      hasEarlier={false}
      loadingEarlier={false}
      historyError={null}
      projectRoots={[]}
      retentionIncomplete={false}
    />,
  );
  expect(await screen.findByText("Edited 33 files")).toBeVisible();
  expect(
    container.querySelector(
      ".acp-edited-files__header .acp-file-changes__totals",
    ),
  ).toBeNull();
});

it("labels source-truncated edit activity as a lower bound and omits partial line totals", async () => {
  const page = snapshotPage();
  const edit = page.turns[0].items[0];
  if (edit.kind !== "fileChangeDiff") throw new Error("Expected edit");
  edit.payload.truncated = true;
  const { container } = render(
    <AcpChatPresentation
      page={page}
      hasEarlier={false}
      loadingEarlier={false}
      historyError={null}
      projectRoots={[]}
      retentionIncomplete={false}
    />,
  );
  expect(await screen.findByText("Edited at least 1 file")).toBeVisible();
  expect(
    container.querySelector(
      ".acp-edited-files__header .acp-file-changes__totals",
    ),
  ).toBeNull();
});

it.each([
  [null, null],
  [0, 2],
  [3, null],
] as const)(
  "shows summary-only Edited activity with truthful counts (%s, %s)",
  async (additions, deletions) => {
    const page = snapshotPage();
    const edit = page.turns[0].items[0];
    if (edit.kind !== "fileChangeDiff") throw new Error("Expected edit");
    const saved = edit.payload.changes[0];
    edit.payload.changes = [
      {
        resource: saved.resource,
        operation: saved.operation,
        originalResource: saved.originalResource,
        summary: saved.summary,
        additions,
        deletions,
      },
    ];
    const { container } = render(
      <AcpChatPresentation
        page={page}
        hasEarlier={false}
        loadingEarlier={false}
        historyError={null}
        projectRoots={[]}
        retentionIncomplete={false}
      />,
    );
    const row = (await screen.findByText("Edited a.ts")).closest(
      ".acp-edited-files",
    )!;
    expect(row).toBeVisible();
    expect(row).not.toHaveTextContent("Saved edit");
    if (additions !== null) expect(row).toHaveTextContent(`+${additions}`);
    if (deletions !== null) expect(row).toHaveTextContent(`-${deletions}`);
    expect(
      row.querySelectorAll(
        ".acp-edited-files__header .acp-file-changes__totals > span",
      ),
    ).toHaveLength(Number(additions !== null) + Number(deletions !== null));
    expect(container.querySelector("details.chat-transcript-work")).toBeNull();
  },
);

it("does not render activity from foreign edit records", () => {
  const page = snapshotPage();
  page.turns[0].items = [
    { ...page.turns[0].items[0], chatId: "foreign" } as never,
  ];
  render(<TurnFileActivity page={page} turn={page.turns[0]} />);
  expect(screen.queryByText(/Edited .* file/)).toBeNull();
});

it("reports an incomplete workspace ledger without an empty edited-files card", () => {
  const page = snapshotPage();
  page.turns[0].items = [];
  render(
    <TurnFileActivity page={page} turn={page.turns[0]} workspaceIncomplete />,
  );
  expect(screen.queryByRole("region", { name: "Edited files" })).toBeNull();
  const notice = screen.getByRole("status");
  expect(notice).toHaveTextContent(
    "Workspace change tracking is incomplete for this turn.",
  );
  expect(screen.queryByText(/Edited at least 0 files/)).toBeNull();
  expect(screen.queryByRole("button")).toBeNull();
});

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
import { AcpChatPresentation } from "./AcpChatPresentation";
import { TurnFileActivity } from "./TurnFileActivity";
import type { AcpItem } from "../../../domain/acp";

afterEach(cleanup);

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
  expect(card).toHaveTextContent("Edited 1 file");
  expect(within(card).getAllByLabelText("Line changes")[0]).toHaveTextContent(
    "+1-1",
  );
  const undo = within(card).getByRole("button", { name: "Undo" });
  expect(undo).toBeDisabled();
  expect(undo).toHaveAttribute(
    "title",
    expect.stringContaining("does not support reverting edits"),
  );
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
    const row = (await screen.findByText("Edited 1 file")).closest(
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

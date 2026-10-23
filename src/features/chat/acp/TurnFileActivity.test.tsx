import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { snapshotPage } from "../../../app/agentDiff.testFixture";
import { AcpChatPresentation } from "./AcpChatPresentation";
import type { AcpItem } from "../../../domain/acp";

afterEach(cleanup);

it("counts all 33 known edited files without presenting retained-subset line totals", () => {
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
  expect(screen.getByText("Edited 33 files")).toBeVisible();
  expect(
    container.querySelector(".acp-edited-files .acp-file-changes__totals"),
  ).toBeNull();
});

it("labels source-truncated edit activity as a lower bound and omits partial line totals", () => {
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
  expect(screen.getByText("Edited at least 1 file")).toBeVisible();
  expect(
    container.querySelector(".acp-edited-files .acp-file-changes__totals"),
  ).toBeNull();
});

it.each([
  [null, null],
  [0, 2],
  [3, null],
] as const)(
  "shows summary-only Edited activity with truthful counts (%s, %s)",
  (additions, deletions) => {
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
    const row = screen.getByText("Edited 1 file").closest(".acp-edited-files")!;
    expect(row).toBeVisible();
    expect(row).not.toHaveTextContent("Saved edit");
    if (additions !== null) expect(row).toHaveTextContent(`+${additions}`);
    if (deletions !== null) expect(row).toHaveTextContent(`-${deletions}`);
    expect(
      row.querySelectorAll(".acp-file-changes__totals > span"),
    ).toHaveLength(Number(additions !== null) + Number(deletions !== null));
    expect(container.querySelector("details.chat-transcript-work")).toBeNull();
  },
);

it("does not render activity from foreign edit records", () => {
  const page = snapshotPage();
  page.turns[0].items = [
    { ...page.turns[0].items[0], chatId: "foreign" } as never,
  ];
  render(
    <AcpChatPresentation
      page={page}
      hasEarlier={false}
      loadingEarlier={false}
      historyError={null}
      projectRoots={[]}
      retentionIncomplete={false}
    />,
  );
  expect(screen.queryByText(/Edited .* file/)).toBeNull();
});

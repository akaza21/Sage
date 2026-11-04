import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { useWorkbenchStore } from "../../app/store";
import { snapshotPage } from "../../app/agentDiff.fixture";
import { ChatChangesSurface, ChangesSurface } from "./ChangesSurface";
import {
  acpProjectionStoreFor,
  clearAcpProjectionRegistry,
} from "../../app/acpProjectionRegistry";

vi.mock("./ChangesFileDiff", () => ({
  ChangesFileDiff: ({
    entry,
  }: {
    entry: { resource: { relativePath: string } };
  }) => <div role="table" aria-label={`Diff ${entry.resource.relativePath}`} />,
}));
const initial = useWorkbenchStore.getState();
afterEach(() => {
  cleanup();
  clearAcpProjectionRegistry();
  useWorkbenchStore.setState(initial, true);
});

it("keeps the staged scope, expanded file and scroll when approval and completion arrive", () => {
  const page = snapshotPage();
  const projection = acpProjectionStoreFor(page.chatId);
  projection.setState({
    page: { ...page, turns: [{ ...page.turns[0], state: "running" }] },
  });
  useWorkbenchStore.setState({
    repositories: {
      roots: [
        {
          rootId: "root-a",
          state: "repository",
          branch: "main",
          elapsedMs: 0,
          truncated: false,
          message: null,
          changes: ["one.ts", "two.ts"].map((relativePath) => ({
            resource: { rootId: "root-a", relativePath },
            indexStatus: "modified",
            worktreeStatus: "unchanged",
            additions: 1,
            deletions: 0,
          })),
        },
      ],
      truncated: false,
      elapsedMs: 0,
    },
  });
  render(<ChatChangesSurface chatId={page.chatId} />);
  fireEvent.change(screen.getByRole("combobox"), {
    target: { value: "staged" },
  });
  fireEvent.click(screen.getByRole("button", { name: /two.ts/ }));
  const surface = screen.getByRole("region", { name: "Chat changes" });
  surface.scrollTop = 120;
  for (const state of ["waitingApproval", "completed"] as const) {
    act(() =>
      projection.setState({
        page: {
          ...page,
          turns: [{ ...page.turns[0], state }],
        },
      }),
    );
    expect(screen.getByRole("combobox")).toHaveValue("staged");
    expect(screen.getByRole("button", { name: /two.ts/ })).toHaveAttribute(
      "aria-expanded",
      "true",
    );
    expect(screen.getByRole("table", { name: "Diff two.ts" })).toBeVisible();
    expect(screen.getByRole("region", { name: "Chat changes" }).scrollTop).toBe(
      120,
    );
  }
});

it("renders Last turn as per-file sections, not a resource picker", () => {
  render(<ChangesSurface page={snapshotPage()} />);
  expect(screen.getByRole("combobox", { name: "Changes scope" })).toHaveValue(
    "lastTurn",
  );
  expect(
    screen.getByRole("region", { name: "Changes src/a.ts" }),
  ).toBeVisible();
  expect(screen.getByRole("table", { name: "Diff src/a.ts" })).toBeVisible();
  fireEvent.click(screen.getByRole("button", { name: /src\/a.ts/ }));
  expect(screen.queryByRole("table")).toBeNull();
});

it("refreshes Last turn edits in place when the observed turn completes", () => {
  const page = snapshotPage();
  const projection = acpProjectionStoreFor(page.chatId);
  projection.setState({
    page: {
      ...page,
      turns: [{ ...page.turns[0], state: "running", items: [] }],
    },
  });
  render(<ChatChangesSurface chatId={page.chatId} />);
  expect(screen.getByText("No changes in this scope.")).toBeVisible();
  act(() => projection.setState({ page }));
  expect(screen.getByRole("table", { name: "Diff src/a.ts" })).toBeVisible();
});

it("offers real Git scopes and mounts at most one file diff", () => {
  useWorkbenchStore.setState({
    repositories: {
      roots: [
        {
          rootId: "root-a",
          state: "repository",
          branch: "main",
          elapsedMs: 0,
          truncated: false,
          message: null,
          changes: ["one.ts", "two.ts"].map((path) => ({
            resource: { rootId: "root-a", relativePath: path },
            indexStatus: "unchanged",
            worktreeStatus: "modified",
            additions: 1,
            deletions: 0,
          })),
        },
      ],
      truncated: false,
      elapsedMs: 0,
    },
  });
  render(<ChangesSurface page={null} />);
  fireEvent.change(screen.getByRole("combobox"), {
    target: { value: "unstaged" },
  });
  expect(screen.getByText(/not attributed to this Chat/)).toBeVisible();
  expect(screen.getAllByRole("table")).toHaveLength(1);
  fireEvent.click(screen.getByRole("button", { name: /two.ts/ }));
  expect(screen.queryByRole("table", { name: "Diff one.ts" })).toBeNull();
  expect(screen.getByRole("table", { name: "Diff two.ts" })).toBeVisible();
  fireEvent.change(screen.getByRole("combobox"), {
    target: { value: "staged" },
  });
  expect(screen.getByText("No changes in this scope.")).toBeVisible();
});

it("disables unavailable Git scopes with a reason", () => {
  useWorkbenchStore.setState({
    repositories: { roots: [], elapsedMs: 0, truncated: false },
  });
  render(<ChangesSurface page={null} />);
  expect(
    screen.getByRole("option", { name: "Staged — Git unavailable" }),
  ).toBeDisabled();
  expect(
    screen.getByRole("option", { name: "Unstaged — Git unavailable" }),
  ).toBeDisabled();
});

it("holds a snapshot without rescanning streamed pages until explicit refresh", () => {
  const page = snapshotPage();
  const view = render(<ChangesSurface page={page} />);
  const next = {
    ...page,
    turns: [
      ...page.turns,
      { ...page.turns[0], id: "new-turn" as never, ordinal: 2, items: [] },
    ],
  };
  view.rerender(<ChangesSurface page={next} />);
  expect(screen.getByRole("table", { name: "Diff src/a.ts" })).toBeVisible();
  fireEvent.click(screen.getByRole("button", { name: "Refresh changes" }));
  expect(screen.queryByRole("table")).toBeNull();
  expect(screen.getByText("No changes in this scope.")).toBeVisible();
});

it("shows complete totals and hides them for unknown or partial counts", () => {
  const page = snapshotPage();
  const item = page.turns[0].items[0];
  if (item.kind !== "fileChangeDiff") throw new Error("Missing edit fixture");
  const first = { ...item.payload.changes[0], additions: 1, deletions: 2 };
  const second = {
    ...first,
    resource: {
      kind: "resource" as const,
      resource: { rootId: "root-a", relativePath: "src/b.ts" },
    },
    additions: 3,
    deletions: 5,
  };
  const withChanges = (
    changes: typeof item.payload.changes,
    truncated = false,
  ) => ({
    ...page,
    turns: [
      {
        ...page.turns[0],
        // Retained before/after pairs admit one File per authenticated Item.
        items: changes.map((change, index) => ({
          ...item,
          id: `${item.id}-${index}` as typeof item.id,
          ordinal: index + 1,
          payload: { ...item.payload, changes: [change], truncated },
        })),
      },
    ],
  });
  const view = render(<ChangesSurface page={withChanges([first, second])} />);
  expect(screen.getByText("+4")).toBeVisible();
  expect(screen.getByText("−7")).toBeVisible();

  view.rerender(
    <ChangesSurface
      page={withChanges([first, { ...second, additions: null }])}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: "Refresh changes" }));
  expect(screen.queryByText("+4")).toBeNull();
  expect(screen.queryByText("−7")).toBeNull();

  view.rerender(<ChangesSurface page={withChanges([first, second], true)} />);
  fireEvent.click(screen.getByRole("button", { name: "Refresh changes" }));
  expect(screen.queryByText("+4")).toBeNull();
  expect(screen.queryByText("−7")).toBeNull();
});

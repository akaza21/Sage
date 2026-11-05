import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import {
  acpProjectionStoreFor,
  clearAcpProjectionRegistry,
} from "../../app/acpProjectionRegistry";
import {
  seedSnapshotOwner,
  snapshotRef,
  snapshotResource,
} from "../../app/agentDiff.fixture";
import { useWorkbenchStore } from "../../app/store";
import type { FileDiff } from "../../domain/workbench";
import { getWorkbenchPorts } from "../../ports/workbenchRuntime";
import { acpDesktop } from "../../services/acpDesktop";
import { ChangesFileDiff } from "./ChangesFileDiff";
import { ChatChangesSurface } from "./ChangesSurface";

const initial = useWorkbenchStore.getState();
beforeEach(() => {
  seedSnapshotOwner();
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  clearAcpProjectionRegistry();
  useWorkbenchStore.setState(initial, true);
});

it("keeps the loaded staged diff and its scroll through approval and completion", async () => {
  const projection = acpProjectionStoreFor(snapshotRef.chatId);
  const page = projection.getState().page!;
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
          changes: ["src/a.ts", "src/b.ts"].map((relativePath) => ({
            resource: { rootId: "root-a", relativePath },
            indexStatus: "modified",
            worktreeStatus: "unchanged",
            additions: 1,
            deletions: 0,
          })),
        },
      ],
      elapsedMs: 0,
      truncated: false,
    },
  });
  vi.spyOn(getWorkbenchPorts().repository, "getFileDiff").mockImplementation(
    async (resource) => ({
      resource,
      hunks: [
        {
          header: "@@ -0,0 +1 @@",
          lines: [
            { kind: "added", oldLine: null, newLine: 1, text: "staged edit" },
          ],
        },
      ],
      binary: false,
      truncated: false,
    }),
  );
  render(<ChatChangesSurface chatId={page.chatId} />);
  fireEvent.change(screen.getByRole("combobox"), {
    target: { value: "staged" },
  });
  fireEvent.click(screen.getByRole("button", { name: /src\/b.ts/ }));
  const table = await screen.findByRole("table", { name: "Unified file diff" });
  table.scrollTop = 75;
  const surface = screen.getByRole("region", { name: "Chat changes" });
  surface.scrollTop = 120;
  for (const state of ["waitingApproval", "completed"] as const) {
    await act(async () =>
      projection.setState({
        page: { ...page, turns: [{ ...page.turns[0], state }] },
      }),
    );
    expect(screen.getByRole("combobox")).toHaveValue("staged");
    expect(screen.getByRole("button", { name: /src\/b.ts/ })).toHaveAttribute(
      "aria-expanded",
      "true",
    );
    expect(
      screen.getByRole("table", { name: "Unified file diff" }).scrollTop,
    ).toBe(75);
    expect(screen.getByRole("region", { name: "Chat changes" }).scrollTop).toBe(
      120,
    );
  }
});

it("loads a saved edit without requiring a pre-existing per-file workbench tab", async () => {
  const read = vi.spyOn(acpDesktop, "getFileSnapshot").mockResolvedValue({
    ...snapshotRef,
    projectionRevision: "1" as never,
    oldText: "old\r\nline\n",
    newText: "new\r\nline\n",
    oldBytes: 10,
    newBytes: 10,
  });
  render(
    <ChangesFileDiff
      entry={{
        resource: snapshotResource,
        diffRef: snapshotRef,
        additions: 1,
        deletions: 1,
      }}
    />,
  );
  expect(
    await screen.findByRole("table", { name: "Unified file diff" }),
  ).toHaveTextContent("new");
  expect(read).toHaveBeenCalledOnce();
  expect(useWorkbenchStore.getState().tabs).toEqual([]);
});

it("loads a scoped Git section without activating a hidden Diff tab", async () => {
  const read = vi
    .spyOn(getWorkbenchPorts().repository, "getFileDiff")
    .mockResolvedValue({
      resource: snapshotResource,
      hunks: [
        {
          header: "@@ -1 +1 @@",
          lines: [
            {
              kind: "added",
              oldLine: null,
              newLine: 1,
              text: "scoped content",
            },
          ],
        },
      ],
      binary: false,
      truncated: false,
    });
  render(
    <ChangesFileDiff
      entry={{
        resource: snapshotResource,
        additions: 1,
        deletions: 0,
        gitScope: "staged",
      }}
    />,
  );
  expect(await screen.findByRole("table")).toHaveTextContent("scoped content");
  expect(read).toHaveBeenCalledWith(snapshotResource, "staged");
  expect(useWorkbenchStore.getState().tabs).toEqual([]);
});

it("does not request a saved edit after unmounting while admission loads", async () => {
  const read = vi.spyOn(acpDesktop, "getFileSnapshot");
  const view = render(
    <ChangesFileDiff
      entry={{
        resource: snapshotResource,
        diffRef: snapshotRef,
        additions: 1,
        deletions: 1,
      }}
    />,
  );
  view.unmount();
  await act(async () => {
    await import("../../app/agentDiffAdmission");
  });
  expect(read).not.toHaveBeenCalled();
});

it("drops a pending Git read after its pane is unmounted", async () => {
  let resolve!: (value: FileDiff) => void;
  const pending = new Promise<FileDiff>((done) => {
    resolve = done;
  });
  vi.spyOn(getWorkbenchPorts().repository, "getFileDiff").mockReturnValue(
    pending,
  );
  const view = render(
    <ChangesFileDiff
      entry={{ resource: snapshotResource, additions: 0, deletions: 0 }}
    />,
  );
  view.unmount();
  await act(async () => {
    resolve({
      resource: snapshotResource,
      hunks: [],
      binary: false,
      truncated: false,
    });
    await pending;
  });
  expect(screen.queryByRole("table")).toBeNull();
});

it("retains an immutable saved diff across equivalent entries and repository saves", async () => {
  const read = vi.spyOn(acpDesktop, "getFileSnapshot").mockResolvedValue({
    ...snapshotRef,
    projectionRevision: "1" as never,
    oldText: "old",
    newText: "saved",
    oldBytes: 3,
    newBytes: 5,
  });
  const entry = {
    resource: snapshotResource,
    diffRef: snapshotRef,
    additions: 1,
    deletions: 1,
  };
  const view = render(<ChangesFileDiff entry={entry} />);
  const table = await screen.findByRole("table");
  act(() =>
    useWorkbenchStore.setState({
      repositories: { ...useWorkbenchStore.getState().repositories },
    }),
  );
  view.rerender(
    <ChangesFileDiff entry={{ ...entry, diffRef: { ...snapshotRef } }} />,
  );
  expect(screen.getByRole("table")).toBe(table);
  expect(screen.queryByText("Loading diff...")).toBeNull();
  await act(async () => {});
  expect(read).toHaveBeenCalledOnce();
});
it("refreshes a live Git diff when content changes with unchanged line counts", async () => {
  const diff = (text: string): FileDiff => ({
    resource: snapshotResource,
    binary: false,
    truncated: false,
    hunks: [
      {
        header: "@@ -0,0 +1 @@",
        lines: [{ kind: "added", oldLine: null, newLine: 1, text }],
      },
    ],
  });
  const read = vi
    .spyOn(getWorkbenchPorts().repository, "getFileDiff")
    .mockResolvedValueOnce(diff("first content"))
    .mockResolvedValueOnce(diff("second content"));
  const entry = {
    resource: snapshotResource,
    gitScope: "staged" as const,
    additions: 1,
    deletions: 0,
  };
  const view = render(<ChangesFileDiff entry={entry} />);
  expect(await screen.findByRole("table")).toHaveTextContent("first content");
  view.rerender(<ChangesFileDiff entry={{ ...entry }} />);
  await vi.waitFor(() =>
    expect(screen.getByRole("table")).toHaveTextContent("second content"),
  );
  expect(read).toHaveBeenCalledTimes(2);
});

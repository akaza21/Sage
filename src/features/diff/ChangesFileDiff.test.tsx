import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { clearAcpProjectionRegistry } from "../../app/acpProjectionRegistry";
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

import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import {
  seedSnapshotOwner,
  snapshotRef,
  snapshotResource,
} from "../../app/agentDiff.fixture";
import { clearAcpProjectionRegistry } from "../../app/acpProjectionRegistry";
import { useWorkbenchStore } from "../../app/store";
import { getWorkbenchPorts } from "../../ports/workbenchRuntime";
import { acpDesktop } from "../../services/acpDesktop";
import { ChangesFileDiff } from "./ChangesFileDiff";
import type { FileDiff } from "../../domain/workbench";

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

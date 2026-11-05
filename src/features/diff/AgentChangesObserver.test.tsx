import { act, cleanup, render, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { setFlagsFromString } from "node:v8";
import { runInNewContext } from "node:vm";
import { seedSnapshotOwner, snapshotPage } from "../../app/agentDiff.fixture";
import { useAgentArtifactFollowStore } from "../../app/agentArtifactFollowStore";
import * as acpDesktop from "../../services/acpDesktop";
import { useAgentChangesState } from "../../workbench/agentChangesState";
import { resetTurnWorkspaceChangesForTest } from "../chat/acp/turnWorkspaceChanges";
import * as workspaceChanges from "../chat/acp/turnWorkspaceChanges";
import * as snapshots from "./changesSnapshot";
import { AgentChangesObserver } from "./AgentChangesObserver";

vi.mock("../../ports/desktopRuntime", () => ({ isTauriRuntime: () => true }));

afterEach(() => {
  cleanup();
  useAgentChangesState.getState().close();
  useAgentArtifactFollowStore.getState().clear();
  resetTurnWorkspaceChangesForTest();
});

it("collects the handled snapshot and ledger while mounted, then reloads for the next completion", async () => {
  setFlagsFromString("--expose-gc");
  const collect = runInNewContext("gc") as () => void;
  setFlagsFromString("--no-expose-gc");
  const projection = seedSnapshotOwner();
  const page = snapshotPage();
  page.turns[0].items = [];
  let snapshot: WeakRef<object> | undefined;
  let ownerPage: WeakRef<object> | undefined;
  let ledger: WeakRef<object> | undefined;
  let ledgerEntries: WeakRef<object> | undefined;
  const makeSnapshot = snapshots.changesSnapshot;
  const snapshotProbe = vi
    .spyOn(snapshots, "changesSnapshot")
    .mockImplementation((page) => {
      const result = makeSnapshot(page);
      if (result) {
        snapshot = new WeakRef(result);
        ownerPage = new WeakRef(result.page);
      }
      return result;
    });
  const loadLedger = workspaceChanges.default;
  const ledgerProbe = vi
    .spyOn(workspaceChanges, "default")
    .mockImplementation(async (...args) => {
      const result = await loadLedger(...args);
      ledger = new WeakRef(result);
      ledgerEntries = new WeakRef(result.changes);
      return result;
    });
  const load = vi
    .spyOn(acpDesktop, "getTurnWorkspaceChanges")
    .mockResolvedValue({
      rootId: "root-a",
      tracked: true,
      truncated: false,
      changes: Array.from({ length: 300 }, (_, index) => ({
        relativePath: `shell-${index}.ts`,
        operation: "updated",
      })),
    });
  projection.setState({ page });
  const view = render(
    <AgentChangesObserver
      page={page}
      initialTurnKey="turn-a:running"
      generation={1}
    />,
  );
  await waitFor(() =>
    expect(useAgentChangesState.getState().owner?.chatId).toBe(page.chatId),
  );
  await act(() => vi.dynamicImportSettled());
  expect(snapshot).toBeDefined();
  expect(ownerPage).toBeDefined();
  expect(ledger).toBeDefined();
  expect(ledgerEntries).toBeDefined();
  // Spy records otherwise keep the real return values alive themselves.
  snapshotProbe.mockClear();
  ledgerProbe.mockClear();
  // WeakRefs keep targets alive until the current job ends. Collect only
  // after passive unmount cleanup and a fresh event-loop task.
  await new Promise((resolve) => setTimeout(resolve, 0));
  collect();
  expect(snapshot!.deref()).toBeUndefined();
  expect(ownerPage!.deref()).toBeUndefined();
  expect(ledger!.deref()).toBeUndefined();
  expect(ledgerEntries!.deref()).toBeUndefined();
  act(() => useAgentChangesState.getState().close());
  view.rerender(
    <AgentChangesObserver
      page={{ ...page }}
      initialTurnKey="turn-a:running"
      generation={1}
    />,
  );
  expect(useAgentChangesState.getState().owner).toBeNull();
  let finish!: (reply: unknown) => void;
  load.mockImplementation(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  const next = {
    ...page,
    turns: [{ ...page.turns[0], id: "turn-b" as never }],
  };
  projection.setState({ page: next });
  view.rerender(
    <AgentChangesObserver
      page={next}
      initialTurnKey="turn-a:running"
      generation={1}
    />,
  );
  expect(useAgentChangesState.getState().owner).toBeNull();
  await waitFor(() => expect(finish).toBeDefined());
  await act(async () =>
    finish({
      rootId: "root-a",
      tracked: true,
      truncated: false,
      changes: [{ relativePath: "next-shell.ts", operation: "deleted" }],
    }),
  );
  await waitFor(() =>
    expect(useAgentChangesState.getState().owner?.chatId).toBe(page.chatId),
  );
});

it("still follows edits that arrive in a later projection of an empty completed Turn", async () => {
  const projection = seedSnapshotOwner();
  const editedPage = { ...snapshotPage(), projectionRevision: "2" as never };
  const page = {
    ...editedPage,
    projectionRevision: "1" as never,
    turns: [{ ...editedPage.turns[0], items: [] }],
  };
  vi.spyOn(acpDesktop, "getTurnWorkspaceChanges").mockResolvedValue({
    rootId: "root-a",
    tracked: true,
    truncated: false,
    changes: [],
  });
  projection.setState({ page });
  const view = render(
    <AgentChangesObserver
      page={page}
      initialTurnKey="turn-a:running"
      generation={1}
    />,
  );
  await act(() => vi.dynamicImportSettled());
  expect(useAgentChangesState.getState().owner).toBeNull();
  projection.setState({ page: editedPage });
  view.rerender(
    <AgentChangesObserver
      page={editedPage}
      initialTurnKey="turn-a:running"
      generation={1}
    />,
  );
  await waitFor(() =>
    expect(useAgentChangesState.getState().owner?.chatId).toBe(page.chatId),
  );
});

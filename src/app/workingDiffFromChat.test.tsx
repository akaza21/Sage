import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import {
  seedSnapshotOwner,
  snapshotPage,
  snapshotResource,
} from "./agentDiff.fixture";
import { clearAcpProjectionRegistry } from "./acpProjectionRegistry";
import { useWorkbenchStore } from "./store";
import { openAgentArtifact } from "./openAgentArtifact";
import { useAgentArtifactAdmissionStore } from "./agentArtifactAdmissionStore";
import { TurnFileActivity } from "../features/chat/acp/TurnFileActivity";
import { DiffSurface } from "../features/diff/DiffSurface";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import { diffCacheKey } from "../domain/diffIdentity";

const initial = useWorkbenchStore.getState();
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  clearAcpProjectionRegistry();
  useAgentArtifactAdmissionStore.getState().clear();
  useWorkbenchStore.setState(initial, true);
});

it("admits and renders the owning nested repository's working Diff", async () => {
  const projection = seedSnapshotOwner();
  const page = projection.getState().page!;
  const resource = { ...snapshotResource, relativePath: "nested/src/a.ts" };
  const repository = { rootId: resource.rootId, relativePath: "nested" };
  vi.spyOn(
    getWorkbenchPorts().repository,
    "resolveResourceRepository",
  ).mockResolvedValue({
    projectId: page.projectId,
    transitionEpoch: 1,
    repository,
  });
  vi.spyOn(
    getWorkbenchPorts().repository,
    "getNestedRepositoryFileDiff",
  ).mockResolvedValue({
    projectId: page.projectId,
    transitionEpoch: 1,
    diff: {
      resource,
      binary: false,
      truncated: false,
      hunks: [
        {
          header: "@@ -1 +1 @@",
          lines: [
            {
              kind: "removed",
              text: "nested before",
              oldLine: 1,
              newLine: null,
            },
            { kind: "added", text: "nested after", oldLine: null, newLine: 1 },
          ],
        },
      ],
    },
  });
  const owner = {
    projectId: page.projectId,
    chatId: page.chatId,
    sessionId: page.currentSessionId!,
  };
  expect(
    await openAgentArtifact({ owner, target: "workingTreeDiff", resource }),
  ).toBe(true);
  const state = useWorkbenchStore.getState();
  const tab = state.tabs.find((tab) => tab.id === state.activeTabId)!;
  expect(tab.repository).toEqual(repository);
  expect(useAgentArtifactAdmissionStore.getState().admissions).toContainEqual({
    owner,
    tabId: tab.id,
  });
  await waitFor(() =>
    expect(
      useWorkbenchStore.getState().diffs[diffCacheKey(repository, resource)]
        ?.phase,
    ).toBe("ready"),
  );
  render(<DiffSurface tab={tab} readOnly />);
  expect(
    await screen.findByRole("table", { name: "Unified file diff" }),
  ).toHaveTextContent("nested after");
});

it.each([true, false])(
  "opens summary-only edits in a per-file Diff and reports baseline availability (%s)",
  async (baseline) => {
    const projection = seedSnapshotOwner();
    const page = projection.getState().page!;
    const edit = page.turns[0].items[0];
    if (edit.kind !== "fileChangeDiff") throw new Error("Expected edit");
    Object.assign(edit.payload.changes[0], {
      pairState: "unavailable",
      snapshotRevision: "0",
      contentDigest: null,
      oldBytes: null,
      newBytes: null,
      additions: null,
      deletions: null,
    });
    // An older summary-only card must still work after a later saved edit of
    // the same resource; its explicit working-tree intent is independent.
    const later = snapshotPage().turns[0];
    page.turns = [
      ...page.turns,
      {
        ...later,
        id: "later-turn" as typeof later.id,
        ordinal: 2,
        items: later.items.map((item) => ({
          ...item,
          id: "later-edit" as typeof item.id,
          turnId: "later-turn" as typeof item.turnId,
        })),
      },
    ];
    const read = vi.spyOn(getWorkbenchPorts().repository, "getFileDiff");
    if (baseline)
      read.mockResolvedValue({
        resource: snapshotResource,
        binary: false,
        truncated: false,
        hunks: [
          {
            header: "@@ -1 +1 @@",
            lines: [
              { kind: "removed", text: "before", oldLine: 1, newLine: null },
              { kind: "added", text: "after", oldLine: null, newLine: 1 },
            ],
          },
        ],
      });
    else read.mockRejectedValue(new Error("This file has no Git baseline."));
    useWorkbenchStore.setState((state) => ({
      tabs: [
        ...state.tabs,
        {
          id: "nested-diff",
          kind: "diff",
          title: "Nested repository diff",
          resource: snapshotResource,
          repository: {
            rootId: snapshotResource.rootId,
            relativePath: "nested",
          },
          position: state.tabs.length,
          pinned: false,
          dirty: false,
        },
      ],
    }));
    render(
      <TurnFileActivity
        page={page}
        turn={page.turns[0]}
        onOpenResource={(resource, target, owner, diffRef) => {
          void openAgentArtifact({ resource, target, owner: owner!, diffRef });
        }}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "View changes" }));
    await waitFor(() =>
      expect(
        useWorkbenchStore
          .getState()
          .tabs.find(
            (tab) => tab.id === useWorkbenchStore.getState().activeTabId,
          )?.kind,
      ).toBe("diff"),
    );
    const tab = useWorkbenchStore
      .getState()
      .tabs.find((tab) => tab.id === useWorkbenchStore.getState().activeTabId)!;
    expect(tab.resource).toEqual(snapshotResource);
    expect(tab.diffRef).toBeUndefined();
    expect(useAgentArtifactAdmissionStore.getState().admissions).toContainEqual(
      {
        owner: {
          projectId: page.projectId,
          chatId: page.chatId,
          sessionId: page.currentSessionId,
        },
        tabId: tab.id,
      },
    );
    render(<DiffSurface tab={tab} readOnly />);
    if (baseline) {
      expect(
        await screen.findByRole("table", { name: "Unified file diff" }),
      ).toHaveTextContent("after");
    } else {
      expect(
        await screen.findByText("This file has no Git baseline."),
      ).toBeVisible();
    }
  },
);

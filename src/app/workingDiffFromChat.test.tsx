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

const initial = useWorkbenchStore.getState();
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  clearAcpProjectionRegistry();
  useAgentArtifactAdmissionStore.getState().clear();
  useWorkbenchStore.setState(initial, true);
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

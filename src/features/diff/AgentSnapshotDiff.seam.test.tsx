import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { clearAcpProjectionRegistry } from "../../app/acpProjectionRegistry";
import { useAgentArtifactAdmissionStore } from "../../app/agentArtifactAdmissionStore";
import { useAgentArtifactFollowStore } from "../../app/agentArtifactFollowStore";
import {
  seedSnapshotOwner,
  snapshotPage,
  snapshotRef as ref,
  snapshotResource as resource,
} from "../../app/agentDiff.testFixture";
import { openAgentArtifact } from "../../app/openAgentArtifact";
import { useWorkbenchStore } from "../../app/store";
import { acpDesktop } from "../../services/acpDesktop";
import { getWorkbenchPorts } from "../../ports/workbenchRuntime";
import { TurnEditSummary } from "../chat/acp/TurnEditSummary";
import { DiffSurface } from "./DiffSurface";

const initial = useWorkbenchStore.getState();
let projection: ReturnType<typeof seedSnapshotOwner>;
beforeEach(() => {
  clearAcpProjectionRegistry();
  projection = seedSnapshotOwner();
  useWorkbenchStore.setState({
    flushActiveEditor: vi.fn(async () => undefined),
    revealFile: vi.fn(async () => undefined),
  });
  vi.spyOn(acpDesktop, "getFileSnapshot").mockResolvedValue({
    ...ref,
    projectionRevision: "1" as never,
    oldText: "old\n",
    newText: "new\n",
    oldBytes: 4,
    newBytes: 4,
  });
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  clearAcpProjectionRegistry();
  useAgentArtifactAdmissionStore.getState().clear();
  useAgentArtifactFollowStore.getState().clear();
  useWorkbenchStore.setState(initial, true);
});
function OpenSurface() {
  const tab = useWorkbenchStore((state) =>
    state.tabs.find((candidate) => candidate.id === state.activeTabId),
  );
  return tab ? <DiffSurface tab={tab} /> : null;
}
describe("actual saved-edit card to shared Diff surface", () => {
  it("never substitutes Git when a restored snapshot placement has lost its reference", async () => {
    const git = vi.spyOn(getWorkbenchPorts().repository, "getFileDiff");
    const tab = {
      id: "diff:snapshot:lost-reference",
      kind: "diff" as const,
      title: "a.ts",
      position: 1,
      pinned: false,
      dirty: false,
      resource,
    };
    useWorkbenchStore.setState({ tabs: [tab], activeTabId: tab.id });
    render(<DiffSurface tab={tab} />);
    await useWorkbenchStore.getState().loadDiff(resource);
    expect(screen.getByRole("alert")).toBeVisible();
    expect(screen.getByText("Saved agent change")).toBeVisible();
    expect(screen.queryByRole("table")).toBeNull();
    expect(git).not.toHaveBeenCalled();
    expect(acpDesktop.getFileSnapshot).not.toHaveBeenCalled();
  });
  it("opens the immutable pair without Git, and hides cached text after session replacement", async () => {
    const git = vi.spyOn(getWorkbenchPorts().repository, "getFileDiff");
    const page = snapshotPage();
    render(
      <>
        <TurnEditSummary
          page={page}
          turn={page.turns[0]}
          rootLabels={{ "root-a": "Root" }}
          countIncomplete={false}
          onOpenDiff={(selected, diffRef) => {
            void openAgentArtifact({
              owner: ref,
              target: "diff",
              resource: selected,
              diffRef,
            });
          }}
        />
        <OpenSurface />
      </>,
    );
    fireEvent.click(
      screen.getByRole("button", { name: "Open diff Root / src/a.ts" }),
    );
    expect(
      await screen.findByRole("table", { name: "Unified file diff" }),
    ).toBeVisible();
    expect(screen.getByText("-old")).toBeVisible();
    expect(screen.getByText("+new")).toBeVisible();
    expect(screen.getByText("Saved agent change")).toBeVisible();
    expect(screen.queryByText("Working tree changes")).toBeNull();
    expect(git).not.toHaveBeenCalled();
    expect(useWorkbenchStore.getState().tabs[0].diffRef).toEqual(ref);
    await waitFor(() =>
      expect(useAgentArtifactAdmissionStore.getState().admissions).toHaveLength(
        1,
      ),
    );
    act(() =>
      projection.setState({
        page: {
          ...projection.getState().page!,
          currentSessionId: "replacement" as never,
        },
      }),
    );
    expect(screen.queryByText("-old")).toBeNull();
    expect(screen.getByRole("alert")).toHaveTextContent("current Chat session");
    expect(acpDesktop.getFileSnapshot).toHaveBeenCalledOnce();
  });
  it("rejects no-ref, foreign owner, forged version and stale selected-item opens", async () => {
    const openDiff = vi.spyOn(useWorkbenchStore.getState(), "openDiff");
    for (const diffRef of [
      undefined,
      { ...ref, snapshotRevision: "2" },
      { ...ref, itemId: "other" as never },
      { ...ref, chatId: "other" as never },
    ]) {
      await expect(
        openAgentArtifact({ owner: ref, target: "diff", resource, diffRef }),
      ).resolves.toBe(false);
    }
    expect(openDiff).not.toHaveBeenCalled();
    expect(acpDesktop.getFileSnapshot).not.toHaveBeenCalled();
  });
  it("does not open an old owner after an asynchronous editor flush", async () => {
    let finish!: () => void;
    const flush = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          finish = resolve;
        }),
    );
    useWorkbenchStore.setState({ flushActiveEditor: flush });
    const opening = openAgentArtifact({
      owner: ref,
      target: "diff",
      resource,
      diffRef: ref,
    });
    await waitFor(() => expect(flush).toHaveBeenCalledOnce());
    projection.setState({
      page: {
        ...projection.getState().page!,
        currentSessionId: "replacement" as never,
      },
    });
    finish();
    await expect(opening).resolves.toBe(false);
    expect(useWorkbenchStore.getState().tabs).toHaveLength(0);
    expect(acpDesktop.getFileSnapshot).not.toHaveBeenCalled();
  });
  it("keeps legacy summary-only edits visible without promising a snapshot action", () => {
    const page = snapshotPage();
    const item = page.turns[0].items[0];
    if (item.kind !== "fileChangeDiff") throw new Error("fixture");
    item.payload.changes = item.payload.changes.map((change) => ({
      ...change,
      pairState: "unavailable",
      snapshotRevision: "0",
      oldBytes: null,
      newBytes: null,
      contentDigest: null,
    }));
    render(
      <TurnEditSummary
        page={page}
        turn={page.turns[0]}
        rootLabels={{ "root-a": "Root" }}
        countIncomplete={false}
        onOpenDiff={vi.fn()}
      />,
    );
    expect(screen.getByText("Saved edit")).toBeVisible();
    expect(screen.queryByRole("button", { name: /Open diff/ })).toBeNull();
  });
});

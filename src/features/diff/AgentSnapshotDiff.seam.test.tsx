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
import { pinAgentChatOwner } from "../../app/agentChatOwner";
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
  pinAgentChatOwner(null);
  useWorkbenchStore.setState(initial, true);
});
function OpenSurface() {
  const tab = useWorkbenchStore((state) =>
    state.tabs.find((candidate) => candidate.id === state.activeTabId),
  );
  return tab ? <DiffSurface tab={tab} /> : null;
}
describe("actual saved-edit card to shared Diff surface", () => {
  it("keeps the chat pill collapsed during a live Turn and preserves explicit disclosure on completion", () => {
    const complete = snapshotPage();
    const runningTurn = { ...complete.turns[0], state: "running" as const };
    const { rerender } = render(
      <TurnEditSummary
        page={{ ...complete, turns: [runningTurn] }}
        rootLabels={{ "root-a": "Root" }}
        countIncomplete={false}
      />,
    );
    expect(
      screen.getByRole("button", { name: /1 file changed/ }),
    ).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByRole("list")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: /1 file changed/ }));
    rerender(
      <TurnEditSummary
        page={complete}
        rootLabels={{ "root-a": "Root" }}
        countIncomplete={false}
      />,
    );
    expect(
      screen.getByRole("button", { name: /1 file changed/ }),
    ).toHaveAttribute("aria-expanded", "true");
    fireEvent.click(screen.getByRole("button", { name: /1 file changed/ }));
    expect(
      screen.getByRole("button", { name: /1 file changed/ }),
    ).toHaveAttribute("aria-expanded", "false");
  });
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
  it("previews a retained saved edit inline without opening a Diff tab or reading Git", async () => {
    const git = vi.spyOn(getWorkbenchPorts().repository, "getFileDiff");
    const openDiff = vi.fn();
    const page = snapshotPage();
    const { rerender } = render(
      <TurnEditSummary
        page={page}
        rootLabels={{ "root-a": "Root" }}
        countIncomplete={false}
        onOpenDiff={openDiff}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /1 file changed/ }));
    expect(
      screen.getByRole("button", { name: "Open diff Root / src/a.ts" }),
    ).toBeVisible();

    fireEvent.click(
      screen.getByRole("button", {
        name: "Preview saved diff Root / src/a.ts",
      }),
    );
    expect(
      await screen.findByRole("table", { name: "Inline saved diff" }),
    ).toBeVisible();
    expect(screen.getByText("-old")).toBeVisible();
    expect(screen.getByText("+new")).toBeVisible();
    expect(acpDesktop.getFileSnapshot).toHaveBeenCalledOnce();
    expect(acpDesktop.getFileSnapshot).toHaveBeenCalledWith({
      ...ref,
      projectGeneration: "1",
      expectedProjectionRevision: "1",
    });
    expect(useWorkbenchStore.getState().tabs).toHaveLength(0);
    expect(openDiff).not.toHaveBeenCalled();
    expect(git).not.toHaveBeenCalled();

    rerender(
      <TurnEditSummary
        page={{ ...page }}
        rootLabels={{ "root-a": "Root" }}
        countIncomplete={false}
        onOpenDiff={openDiff}
      />,
    );
    expect(acpDesktop.getFileSnapshot).toHaveBeenCalledOnce();
    fireEvent.click(screen.getByRole("button", { name: /1 file changed/ }));
    expect(
      screen.queryByRole("table", { name: "Inline saved diff" }),
    ).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: /1 file changed/ }));
    expect(
      screen.queryByRole("table", { name: "Inline saved diff" }),
    ).toBeNull();
  });
  it("refuses an inline read when the saved edit is no longer retained by its Chat", async () => {
    const page = snapshotPage();
    act(() =>
      projection.setState({
        page: { ...page, currentSessionId: "replacement" as never },
      }),
    );
    render(
      <TurnEditSummary
        page={page}
        rootLabels={{ "root-a": "Root" }}
        countIncomplete={false}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /1 file changed/ }));
    fireEvent.click(
      screen.getByRole("button", {
        name: "Preview saved diff Root / src/a.ts",
      }),
    );
    expect(await screen.findByRole("alert")).toHaveTextContent("unavailable");
    expect(acpDesktop.getFileSnapshot).not.toHaveBeenCalled();
  });
  it("discards a late inline snapshot after the Chat session changes", async () => {
    let resolveRead!: (
      value: Awaited<ReturnType<typeof acpDesktop.getFileSnapshot>>,
    ) => void;
    vi.mocked(acpDesktop.getFileSnapshot).mockImplementation(
      () =>
        new Promise((resolve) => {
          resolveRead = resolve;
        }),
    );
    const page = snapshotPage();
    render(
      <TurnEditSummary
        page={page}
        rootLabels={{ "root-a": "Root" }}
        countIncomplete={false}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /1 file changed/ }));
    fireEvent.click(
      screen.getByRole("button", {
        name: "Preview saved diff Root / src/a.ts",
      }),
    );
    expect(screen.getByRole("status")).toHaveTextContent("Loading saved diff");
    await waitFor(() =>
      expect(acpDesktop.getFileSnapshot).toHaveBeenCalledOnce(),
    );
    act(() =>
      projection.setState({
        page: { ...page, currentSessionId: "replacement" as never },
      }),
    );
    await act(async () =>
      resolveRead({
        ...ref,
        projectionRevision: "1" as never,
        oldText: "old\n",
        newText: "new\n",
        oldBytes: 4,
        newBytes: 4,
      }),
    );
    expect(
      screen.queryByRole("table", { name: "Inline saved diff" }),
    ).toBeNull();
    expect(screen.getByRole("alert")).toHaveTextContent("unavailable");
  });
  it("settles an in-flight preview when its projection advances and retries explicitly", async () => {
    let resolveRead!: (
      value: Awaited<ReturnType<typeof acpDesktop.getFileSnapshot>>,
    ) => void;
    vi.mocked(acpDesktop.getFileSnapshot)
      .mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            resolveRead = resolve;
          }),
      )
      .mockResolvedValueOnce({
        ...ref,
        projectionRevision: "2" as never,
        oldText: "old\n",
        newText: "current\n",
        oldBytes: 4,
        newBytes: 8,
      });
    const page = snapshotPage();
    render(
      <TurnEditSummary
        page={page}
        rootLabels={{ "root-a": "Root" }}
        countIncomplete={false}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /1 file changed/ }));
    fireEvent.click(
      screen.getByRole("button", {
        name: "Preview saved diff Root / src/a.ts",
      }),
    );
    await waitFor(() =>
      expect(acpDesktop.getFileSnapshot).toHaveBeenCalledOnce(),
    );
    act(() =>
      projection.setState({
        page: { ...page, projectionRevision: "2" as never },
      }),
    );
    expect(screen.getByRole("alert")).toHaveTextContent("Retry");
    expect(acpDesktop.getFileSnapshot).toHaveBeenCalledOnce();
    await act(async () =>
      resolveRead({
        ...ref,
        projectionRevision: "1" as never,
        oldText: "old\n",
        newText: "stale\n",
        oldBytes: 4,
        newBytes: 6,
      }),
    );
    expect(screen.queryByText("+stale")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Retry saved diff" }));
    expect(await screen.findByText("+current")).toBeVisible();
    expect(acpDesktop.getFileSnapshot).toHaveBeenCalledTimes(2);
    expect(acpDesktop.getFileSnapshot).toHaveBeenLastCalledWith(
      expect.objectContaining({ expectedProjectionRevision: "2" }),
    );
  });
  it("hides ready preview bytes when its projection advances until explicit retry", async () => {
    const page = snapshotPage();
    render(
      <TurnEditSummary
        page={page}
        rootLabels={{ "root-a": "Root" }}
        countIncomplete={false}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /1 file changed/ }));
    fireEvent.click(
      screen.getByRole("button", {
        name: "Preview saved diff Root / src/a.ts",
      }),
    );
    expect(await screen.findByText("+new")).toBeVisible();
    act(() =>
      projection.setState({
        page: { ...page, projectionRevision: "2" as never },
      }),
    );
    expect(screen.queryByText("+new")).toBeNull();
    expect(
      screen.getByRole("button", { name: "Retry saved diff" }),
    ).toBeVisible();
    expect(acpDesktop.getFileSnapshot).toHaveBeenCalledOnce();
  });
  it("offers an explicit retry after an inline snapshot error", async () => {
    vi.mocked(acpDesktop.getFileSnapshot)
      .mockRejectedValueOnce(new Error("Snapshot unavailable"))
      .mockResolvedValueOnce({
        ...ref,
        projectionRevision: "1" as never,
        oldText: "old\n",
        newText: "new\n",
        oldBytes: 4,
        newBytes: 4,
      });
    const page = snapshotPage();
    render(
      <TurnEditSummary
        page={page}
        rootLabels={{ "root-a": "Root" }}
        countIncomplete={false}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /1 file changed/ }));
    fireEvent.click(
      screen.getByRole("button", {
        name: "Preview saved diff Root / src/a.ts",
      }),
    );
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Snapshot unavailable",
    );
    fireEvent.click(screen.getByRole("button", { name: "Retry saved diff" }));
    expect(
      await screen.findByRole("table", { name: "Inline saved diff" }),
    ).toBeVisible();
    expect(acpDesktop.getFileSnapshot).toHaveBeenCalledTimes(2);
  });
  it("limits inline rows while leaving the full saved Diff action available", async () => {
    const oldText = Array.from(
      { length: 150 },
      (_, index) => `old-${index}\n`,
    ).join("");
    const newText = Array.from(
      { length: 150 },
      (_, index) => `new-${index}\n`,
    ).join("");
    vi.mocked(acpDesktop.getFileSnapshot).mockResolvedValue({
      ...ref,
      projectionRevision: "1" as never,
      oldText,
      newText,
      oldBytes: oldText.length,
      newBytes: newText.length,
    });
    const page = snapshotPage();
    render(
      <TurnEditSummary
        page={page}
        rootLabels={{ "root-a": "Root" }}
        countIncomplete={false}
        onOpenDiff={vi.fn()}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /1 file changed/ }));
    fireEvent.click(
      screen.getByRole("button", {
        name: "Preview saved diff Root / src/a.ts",
      }),
    );
    const table = await screen.findByRole("table", {
      name: "Inline saved diff",
    });
    expect(table.querySelectorAll('[role="row"]')).toHaveLength(121);
    expect(screen.getByText(/Preview limited; open full diff/)).toBeVisible();
    expect(
      screen.getByRole("button", { name: "Open diff Root / src/a.ts" }),
    ).toBeVisible();
  });
  it("opens the immutable pair without Git, and hides cached text after session replacement", async () => {
    const git = vi.spyOn(getWorkbenchPorts().repository, "getFileDiff");
    const page = snapshotPage();
    render(
      <>
        <TurnEditSummary
          page={page}
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
    const editPill = screen.getByRole("button", { name: /1 file changed/ });
    expect(editPill).toHaveAttribute("aria-expanded", "false");
    expect(
      screen.queryByRole("button", { name: "Open diff Root / src/a.ts" }),
    ).toBeNull();
    fireEvent.click(editPill);
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
  it("adds a line comment only to the exact pinned Chat composer", async () => {
    const page = snapshotPage();
    useWorkbenchStore.setState({
      tabs: [
        {
          id: ref.chatId,
          kind: "chat",
          title: page.title,
          position: 0,
          pinned: true,
          dirty: false,
        },
      ],
      activeTabId: ref.chatId,
      chatOwnershipEpoch: 1,
      chatUiById: {
        [ref.chatId]: {
          composerText: "Fix this",
          attachedContext: [],
          sending: false,
          error: null,
          errorKind: null,
          requestGeneration: 0,
        },
      },
    });
    await openAgentArtifact({
      owner: ref,
      target: "diff",
      resource,
      diffRef: ref,
    });
    pinAgentChatOwner(ref.chatId);

    const diffTab = useWorkbenchStore
      .getState()
      .tabs.find((tab) => tab.kind === "diff")!;
    const collapsePane = vi.fn();
    render(
      <div onKeyDown={collapsePane}>
        <DiffSurface tab={diffTab} readOnly />
      </div>,
    );
    expect(
      await screen.findByRole("table", { name: "Unified file diff" }),
    ).toBeVisible();
    fireEvent.click(
      screen.getByRole("button", { name: "Comment on old line 1" }),
    );
    const input = screen.getByRole("textbox", {
      name: "Comment on old line 1",
    });
    fireEvent.keyDown(input, { key: "Escape" });
    expect(collapsePane).not.toHaveBeenCalled();
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "Comment on old line 1" }),
      ).toHaveFocus(),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "Comment on old line 1" }),
    );
    fireEvent.change(
      screen.getByRole("textbox", { name: "Comment on old line 1" }),
      { target: { value: "Keep the old guard" } },
    );
    fireEvent.click(screen.getByRole("button", { name: "Add to prompt" }));

    expect(
      useWorkbenchStore.getState().chatUiById[ref.chatId].attachedContext,
    ).toEqual([
      expect.objectContaining({
        kind: "diffLineComment",
        diffRef: ref,
        side: "old",
        startLine: "1",
        endLine: "1",
        note: "Keep the old guard",
      }),
    ]);
    const owner = useWorkbenchStore
      .getState()
      .captureChatOwnership(ref.chatId)!;
    act(() => {
      useWorkbenchStore.getState().setChatActionState(owner, {
        sending: true,
        error: null,
      });
    });
    expect(
      screen.queryAllByRole("button", { name: /Comment on/ }),
    ).toHaveLength(0);
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
        rootLabels={{ "root-a": "Root" }}
        countIncomplete={false}
        onOpenDiff={vi.fn()}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /1 file changed/ }));
    expect(screen.getByText("Saved edit")).toBeVisible();
    expect(screen.queryByRole("button", { name: /Open diff/ })).toBeNull();
  });
});

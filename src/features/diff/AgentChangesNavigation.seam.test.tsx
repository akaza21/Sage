import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import {
  seedSnapshotOwner,
  snapshotPage,
  snapshotRef as ref,
  snapshotResource as resource,
} from "../../app/agentDiff.fixture";
import { clearAcpProjectionRegistry } from "../../app/acpProjectionRegistry";
import { pinAgentChatOwner } from "../../app/agentChatOwner";
import { useWorkbenchStore } from "../../app/store";
import { useAgentArtifactAdmissionStore } from "../../app/agentArtifactAdmissionStore";
import {
  agentArtifactFollowRecordFor,
  useAgentArtifactFollowStore,
} from "../../app/agentArtifactFollowStore";
import { activeArtifactForTarget } from "../../workbench/agentArtifactTabPresentation";
import { getWorkbenchPorts } from "../../ports/workbenchRuntime";
import { acpDesktop } from "../../services/acpDesktop";
import { AgentArtifactHeader } from "../../workbench/AgentArtifactHeader";
import { useAgentChangesState } from "../../workbench/agentChangesState";
import { openAgentSnapshotSource } from "../../app/agentDiffSource";
import { openAgentArtifact } from "../../app/openAgentArtifact";
import { acpProjectionStoreFor } from "../../app/acpProjectionRegistry";
import { ChangesSurface } from "./ChangesSurface";
import { DiffSurface } from "./DiffSurface";
import { FileSurface } from "../files/FileSurface";

const initial = useWorkbenchStore.getState();
const nativeEditor = getWorkbenchPorts().editor;
let diskText = "today's live source\n";

beforeEach(() => {
  diskText = "today's live source\n";
  seedSnapshotOwner();
  useWorkbenchStore.setState({
    tabs: [
      {
        id: ref.chatId,
        kind: "chat",
        title: "Chat",
        dirty: false,
        pinned: false,
        position: 0,
      },
    ],
    activeTabId: ref.chatId,
    chatOwnershipEpoch: 1,
  });
  pinAgentChatOwner(ref.chatId);
  useAgentChangesState.getState().show(ref, 1);
  vi.spyOn(acpDesktop, "getFileSnapshot").mockResolvedValue({
    ...ref,
    projectionRevision: "1" as never,
    oldText: "saved old\n",
    newText: "saved new\n",
    oldBytes: 10,
    newBytes: 10,
  });
  vi.spyOn(nativeEditor, "openEditorDocument").mockImplementation(async () => {
    const version = {
      fingerprint: "disk",
      byteLen: diskText.length,
      modifiedAt: null,
    };
    return {
      resource,
      disk: version,
      base: version,
      buffer: {
        fingerprint: "disk",
        byteLen: diskText.length,
        revision: 0,
        content: diskText,
      },
      lineEnding: "lf",
      dirty: false,
      conflict: null,
    };
  });
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      disconnect() {}
    },
  );
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  pinAgentChatOwner(null);
  useAgentChangesState.getState().close();
  useAgentArtifactAdmissionStore.getState().clear();
  useAgentArtifactFollowStore.getState().clear();
  clearAcpProjectionRegistry();
  useWorkbenchStore.setState(initial, true);
});

function Canvas() {
  const tabs = useWorkbenchStore((state) => state.tabs);
  const records = useAgentArtifactFollowStore((state) => state.records);
  const tab = activeArtifactForTarget(
    tabs,
    agentArtifactFollowRecordFor(records, ref)?.selected ?? null,
  );
  if (!tab || tab.kind === "chat") return null;
  return (
    <>
      <AgentArtifactHeader
        key={tab.id}
        tab={tab}
        admitted
        page={snapshotPage()}
        position={0}
        total={2}
        onStep={() => undefined}
        onOpenInEditor={() => undefined}
      />
      {tab.kind === "diff" ? (
        <DiffSurface tab={tab} readOnly />
      ) : (
        <FileSurface tab={tab} readOnly />
      )}
    </>
  );
}

it("opens Last turn as a full saved diff, keeps line-comment identity, and returns from changed live source to the original bytes", async () => {
  render(
    <>
      <ChangesSurface page={snapshotPage()} />
      <Canvas />
    </>,
  );
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Open diff" }));
    await vi.dynamicImportSettled();
  });
  await screen.findByRole("button", { name: "Comment on old line 1" });
  fireEvent.click(
    screen.getByRole("button", { name: "Comment on old line 1" }),
  );
  fireEvent.change(
    screen.getByRole("textbox", { name: "Comment on old line 1" }),
    { target: { value: "Preserve saved guard" } },
  );
  fireEvent.click(screen.getByRole("button", { name: "Add to prompt" }));
  expect(
    useWorkbenchStore.getState().chatUiById[ref.chatId].attachedContext,
  ).toEqual([
    expect.objectContaining({
      diffRef: ref,
      startLine: "1",
      side: "old",
      note: "Preserve saved guard",
    }),
  ]);
  await act(async () => {
    fireEvent.click(
      screen.getAllByRole("button", { name: "View live source" })[0],
    );
    await vi.dynamicImportSettled();
  });
  await screen.findByRole("button", { name: "Changes" });
  expect(useWorkbenchStore.getState().activeEditor).toMatchObject({
    phase: "ready",
    content: "today's live source\n",
  });
  diskText = "changed again on disk\n";
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Changes" }));
    await vi.dynamicImportSettled();
  });
  await waitFor(() =>
    expect(
      useWorkbenchStore
        .getState()
        .tabs.find((tab) => tab.id === useWorkbenchStore.getState().activeTabId)
        ?.diffRef,
    ).toEqual(ref),
  );
  expect(
    screen.getAllByRole("table", { name: "Unified file diff" }).at(-1),
  ).toHaveTextContent("saved old");
  expect(
    screen.getAllByRole("table", { name: "Unified file diff" }).at(-1),
  ).toHaveTextContent("saved new");
  expect(
    screen.getAllByRole("table", { name: "Unified file diff" }).at(-1),
  ).not.toHaveTextContent("changed again");
});

it.each(["staged", "unstaged"] as const)(
  "opens a full %s Git diff with the selected scope's bytes",
  async (scope) => {
    vi.spyOn(getWorkbenchPorts().repository, "getFileDiff").mockImplementation(
      async (selected, selectedScope) => ({
        resource: selected,
        binary: false,
        truncated: false,
        hunks: [
          {
            header: "@@",
            lines: [
              {
                kind: "added",
                oldLine: null,
                newLine: 1,
                text:
                  selectedScope === "staged"
                    ? "INDEX BYTES"
                    : selectedScope === "unstaged"
                      ? "WORKTREE BYTES"
                      : "HEAD COMPARISON INCLUDING ALL EDITS",
              },
            ],
          },
        ],
      }),
    );
    useWorkbenchStore.setState({
      repositories: {
        roots: [
          {
            rootId: resource.rootId,
            state: "repository",
            branch: "main",
            message: null,
            elapsedMs: 0,
            truncated: false,
            changes: [
              {
                resource,
                indexStatus: "modified",
                worktreeStatus: "modified",
                additions: null,
                deletions: null,
              },
            ],
          },
        ],
        elapsedMs: 0,
        truncated: false,
      },
    });
    render(
      <>
        <ChangesSurface page={snapshotPage()} />
        <Canvas />
      </>,
    );
    fireEvent.change(screen.getByRole("combobox", { name: "Changes scope" }), {
      target: { value: scope },
    });
    await act(async () =>
      fireEvent.click(screen.getByRole("button", { name: "Open diff" })),
    );
    await waitFor(() =>
      expect(
        screen.getAllByRole("table", { name: "Unified file diff" }).at(-1),
      ).toHaveTextContent(
        scope === "staged" ? "INDEX BYTES" : "WORKTREE BYTES",
      ),
    );
    expect(
      await screen.findByText(
        scope === "staged" ? "Staged changes" : "Unstaged changes",
      ),
    ).toBeVisible();
    const store = useWorkbenchStore.getState();
    act(() =>
      useWorkbenchStore.setState({
        tabs: [
          ...store.tabs,
          {
            id: "unrelated",
            kind: "file",
            title: "other.ts",
            resource: { rootId: resource.rootId, relativePath: "other.ts" },
            position: 10,
            dirty: false,
            pinned: false,
          },
        ],
      }),
    );
    act(() => useWorkbenchStore.getState().closeTab("unrelated"));
    await waitFor(() =>
      expect(
        screen.getAllByRole("table", { name: "Unified file diff" }).at(-1),
      ).toHaveTextContent(
        scope === "staged" ? "INDEX BYTES" : "WORKTREE BYTES",
      ),
    );
    expect(
      screen.getByText(
        scope === "staged" ? "Staged changes" : "Unstaged changes",
      ),
    ).toBeVisible();
    cleanup();
    // Native layout restoration returns the persisted resource identity and
    // clears disposable WebView state. The comparison must still be scoped.
    const scopedTab = useWorkbenchStore
      .getState()
      .tabs.find((tab) => tab.id === useWorkbenchStore.getState().activeTabId)!;
    const {
      id,
      kind,
      title,
      position,
      pinned,
      dirty,
      resource: savedResource,
    } = scopedTab;
    const restored = {
      id,
      kind,
      title,
      position,
      pinned,
      dirty,
      resource: savedResource,
    };
    useWorkbenchStore.setState({ tabs: [restored], projectGeneration: 2 });
    render(<DiffSurface tab={restored} />);
    await waitFor(() =>
      expect(
        screen.getByRole("table", { name: "Unified file diff" }),
      ).toHaveTextContent(
        scope === "staged" ? "INDEX BYTES" : "WORKTREE BYTES",
      ),
    );
    cleanup();
    await act(async () => useWorkbenchStore.getState().openDiff(resource));
    const editorTab = useWorkbenchStore
      .getState()
      .tabs.find((tab) => tab.id === useWorkbenchStore.getState().activeTabId)!;
    render(<DiffSurface tab={editorTab} />);
    await waitFor(() =>
      expect(
        screen.getByRole("table", { name: "Unified file diff" }),
      ).toHaveTextContent("HEAD COMPARISON INCLUDING ALL EDITS"),
    );
    expect(
      screen.queryByText(
        scope === "staged" ? "Staged changes" : "Unstaged changes",
      ),
    ).toBeNull();
  },
);

it("keeps separate Changes return addresses when two Chats view the same shared live File", async () => {
  const firstPage = snapshotPage();
  const secondPage = JSON.parse(
    JSON.stringify(firstPage)
      .replaceAll("chat-a", "chat-b")
      .replaceAll("session-a", "session-b")
      .replaceAll("turn-a", "turn-b")
      .replaceAll("item-a", "item-b"),
  ) as typeof firstPage;
  const secondRef = {
    ...ref,
    chatId: secondPage.chatId,
    sessionId: secondPage.currentSessionId!,
    turnId: secondPage.turns[0].id,
    itemId: secondPage.turns[0].items[0].id,
  };
  const scope = {
    projectId: secondPage.projectId,
    chatId: secondPage.chatId,
    projectGeneration: "1" as never,
  };
  const projection = acpProjectionStoreFor(secondPage.chatId);
  projection.getState().activateScope(scope);
  projection.getState().applyAuthoritativePage(scope, secondPage);
  await openAgentArtifact({
    owner: ref,
    target: "diff",
    resource,
    diffRef: ref,
  });
  expect(await openAgentSnapshotSource(ref)).toBe(true);
  await openAgentArtifact({
    owner: secondRef,
    target: "diff",
    resource,
    diffRef: secondRef,
  });
  expect(await openAgentSnapshotSource(secondRef)).toBe(true);
  const sharedFile = useWorkbenchStore
    .getState()
    .tabs.find((tab) => tab.kind === "file")!;
  const header = {
    tab: sharedFile,
    admitted: true,
    position: 0,
    total: 1,
    onStep: () => undefined,
    onOpenInEditor: () => undefined,
  };
  const view = render(<AgentArtifactHeader {...header} page={firstPage} />);
  fireEvent.click(screen.getByRole("button", { name: "Changes" }));
  await waitFor(() =>
    expect(
      useWorkbenchStore
        .getState()
        .tabs.find((tab) => tab.id === useWorkbenchStore.getState().activeTabId)
        ?.diffRef,
    ).toEqual(ref),
  );
  view.rerender(<AgentArtifactHeader {...header} page={secondPage} />);
  fireEvent.click(screen.getByRole("button", { name: "Changes" }));
  await waitFor(() =>
    expect(
      useWorkbenchStore
        .getState()
        .tabs.find((tab) => tab.id === useWorkbenchStore.getState().activeTabId)
        ?.diffRef,
    ).toEqual(secondRef),
  );
});

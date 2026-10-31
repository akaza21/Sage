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
import { useAgentArtifactFollowStore } from "../../app/agentArtifactFollowStore";
import { getWorkbenchPorts } from "../../ports/workbenchRuntime";
import { acpDesktop } from "../../services/acpDesktop";
import { AgentArtifactHeader } from "../../workbench/AgentArtifactHeader";
import { useAgentChangesState } from "../../workbench/agentChangesState";
import { useAgentChangesScopes } from "../../workbench/agentChangesStateScopes";
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
  useAgentChangesScopes.setState({ gitScopes: [] });
  useAgentArtifactAdmissionStore.getState().clear();
  useAgentArtifactFollowStore.getState().clear();
  clearAcpProjectionRegistry();
  useWorkbenchStore.setState(initial, true);
});

function Canvas() {
  const tab = useWorkbenchStore((state) =>
    state.tabs.find((item) => item.id === state.activeTabId),
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
  fireEvent.click(screen.getByRole("button", { name: "Open diff" }));
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
  fireEvent.click(
    screen.getAllByRole("button", { name: "View live source" })[0],
  );
  await screen.findByRole("button", { name: "Changes" });
  expect(useWorkbenchStore.getState().activeEditor).toMatchObject({
    phase: "ready",
    content: "today's live source\n",
  });
  diskText = "changed again on disk\n";
  fireEvent.click(screen.getByRole("button", { name: "Changes" }));
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
                  selectedScope === "staged" ? "INDEX BYTES" : "WORKTREE BYTES",
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

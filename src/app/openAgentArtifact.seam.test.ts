import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  acpProjectionStoreFor,
  clearAcpProjectionRegistry,
} from "./acpProjectionRegistry";
import { useAgentArtifactAdmissionStore } from "./agentArtifactAdmissionStore";
import {
  agentArtifactFollowRecordFor,
  useAgentArtifactFollowStore,
} from "./agentArtifactFollowStore";
import { openAgentArtifact } from "./openAgentArtifact";
import { useWorkbenchStore } from "./store";
import type { AcpChatPage, ProjectGeneration } from "../domain/acp";
import type { AgentArtifactOwner } from "../domain/agentArtifact";
import type { ResourceRef, WorkbenchTab } from "../domain/workbench";

const initialState = useWorkbenchStore.getState();
const resource: ResourceRef = { rootId: "root-a", relativePath: "src/app.ts" };
const ownerA = {
  projectId: "project-a",
  chatId: "chat-a",
  sessionId: "session-a",
} as AgentArtifactOwner;
const ownerB = {
  projectId: "project-a",
  chatId: "chat-b",
  sessionId: "session-b",
} as AgentArtifactOwner;

function page(owner: AgentArtifactOwner): AcpChatPage {
  return {
    projectId: owner.projectId,
    chatId: owner.chatId,
    title: owner.chatId,
    projectionRevision: "1",
    currentSessionId: owner.sessionId,
    sessions: [],
    turns: [],
    nextCursor: null,
  } as unknown as AcpChatPage;
}

function seedOwner(owner: AgentArtifactOwner): void {
  const store = acpProjectionStoreFor(owner.chatId);
  const scope = {
    projectId: owner.projectId,
    projectGeneration: "1" as ProjectGeneration,
    chatId: owner.chatId,
  };
  store.getState().activateScope(scope);
  expect(store.getState().applyAuthoritativePage(scope, page(owner))).toBe(
    "accepted",
  );
}

function fileTab(): WorkbenchTab {
  return {
    id: "file:root-a:src/app.ts",
    kind: "file",
    title: "app.ts",
    position: 1,
    pinned: false,
    dirty: false,
    resource,
  };
}

beforeEach(() => {
  clearAcpProjectionRegistry();
  useAgentArtifactAdmissionStore.getState().clear();
  useAgentArtifactFollowStore.getState().clear();
  seedOwner(ownerA);
  seedOwner(ownerB);
  useWorkbenchStore.setState({
    project: { id: "project-a", name: "A", roots: [] },
    projectGeneration: 1,
    projectTransitioning: false,
    tabs: [],
    activeTabId: null,
  });
});

afterEach(() => {
  useAgentArtifactAdmissionStore.getState().clear();
  useAgentArtifactFollowStore.getState().clear();
  clearAcpProjectionRegistry();
  useWorkbenchStore.setState(initialState, true);
});

describe("openAgentArtifact", () => {
  it("shares one Editor tab while keeping independent Chat admissions", async () => {
    const openFile = vi.fn(async () => {
      useWorkbenchStore.setState({
        tabs: [fileTab()],
        activeTabId: fileTab().id,
      });
    });
    useWorkbenchStore.setState({ openFile });

    await expect(
      openAgentArtifact({ owner: ownerA, target: "file", resource }),
    ).resolves.toBe(true);
    await expect(
      openAgentArtifact({ owner: ownerB, target: "file", resource }),
    ).resolves.toBe(true);

    expect(useWorkbenchStore.getState().tabs).toHaveLength(1);
    expect(useAgentArtifactAdmissionStore.getState().admissions).toEqual([
      { owner: ownerA, tabId: fileTab().id },
      { owner: ownerB, tabId: fileTab().id },
    ]);
    expect(
      agentArtifactFollowRecordFor(
        useAgentArtifactFollowStore.getState().records,
        ownerA,
      ),
    ).toMatchObject({
      mode: "follow",
      selected: { kind: "tab", tabId: fileTab().id },
    });
    expect(
      agentArtifactFollowRecordFor(
        useAgentArtifactFollowStore.getState().records,
        ownerB,
      ),
    ).toMatchObject({
      mode: "follow",
      selected: { kind: "tab", tabId: fileTab().id },
    });
  });

  it("keeps the deliberate selection pinned when a later File arrives", async () => {
    const firstResource = resource;
    const secondResource = {
      rootId: "root-a",
      relativePath: "src/new file.ts",
    };
    useWorkbenchStore.setState({
      openFile: vi.fn(async (selected) => {
        const tab = {
          id: `file:${selected.rootId}:${selected.relativePath}`,
          kind: "file" as const,
          title: selected.relativePath,
          position: useWorkbenchStore.getState().tabs.length,
          pinned: false,
          dirty: false,
          resource: selected,
        };
        useWorkbenchStore.setState((current) => ({
          tabs: [
            ...current.tabs.filter((candidate) => candidate.id !== tab.id),
            tab,
          ],
          activeTabId: tab.id,
        }));
      }),
    });

    await expect(
      openAgentArtifact({
        owner: ownerA,
        target: "file",
        resource: firstResource,
      }),
    ).resolves.toBe(true);
    const firstTarget = { kind: "tab" as const, tabId: fileTab().id };
    useAgentArtifactFollowStore.getState().pin(ownerA, firstTarget);
    await expect(
      openAgentArtifact({
        owner: ownerA,
        target: "file",
        resource: secondResource,
      }),
    ).resolves.toBe(true);

    expect(
      agentArtifactFollowRecordFor(
        useAgentArtifactFollowStore.getState().records,
        ownerA,
      ),
    ).toMatchObject({
      mode: "pinned",
      selected: firstTarget,
      pending: [{ kind: "tab", tabId: "file:root-a:src/new file.ts" }],
    });
  });

  it("rejects a foreign Project or stale session before opening", async () => {
    const openFile = vi.fn(async () => undefined);
    useWorkbenchStore.setState({ openFile });

    await expect(
      openAgentArtifact({
        owner: { ...ownerA, projectId: "project-other" } as AgentArtifactOwner,
        target: "file",
        resource,
      }),
    ).resolves.toBe(false);
    await expect(
      openAgentArtifact({
        owner: { ...ownerA, sessionId: "session-old" } as AgentArtifactOwner,
        target: "file",
        resource,
      }),
    ).resolves.toBe(false);
    expect(openFile).not.toHaveBeenCalled();
    expect(useAgentArtifactFollowStore.getState().records).toEqual([]);
  });

  it("rejects a cached projection page from an older Project generation", async () => {
    const openFile = vi.fn(async () => undefined);
    useWorkbenchStore.setState({ openFile, projectGeneration: 2 });

    await expect(
      openAgentArtifact({ owner: ownerA, target: "file", resource }),
    ).resolves.toBe(false);

    expect(openFile).not.toHaveBeenCalled();
  });

  it("does not admit a result when ownership changes during the async open", async () => {
    let finishOpen!: () => void;
    const opening = new Promise<void>((resolve) => {
      finishOpen = resolve;
    });
    useWorkbenchStore.setState({
      openFile: vi.fn(async () => {
        await opening;
        useWorkbenchStore.setState({ tabs: [fileTab()] });
      }),
    });

    const result = openAgentArtifact({
      owner: ownerA,
      target: "file",
      resource,
    });
    const store = acpProjectionStoreFor(ownerA.chatId);
    store.setState({
      page: page({ ...ownerA, sessionId: "session-new" } as AgentArtifactOwner),
    });
    finishOpen();

    await expect(result).resolves.toBe(false);
    expect(useAgentArtifactAdmissionStore.getState().admissions).toEqual([]);
    expect(useAgentArtifactFollowStore.getState().records).toEqual([]);
  });

  it("forgets every owner record when a shared resource tab closes", () => {
    const tab = fileTab();
    useWorkbenchStore.setState({ tabs: [tab], activeTabId: tab.id });
    useAgentArtifactAdmissionStore.getState().admit({
      owner: ownerA,
      tabId: tab.id,
    });
    useAgentArtifactAdmissionStore.getState().admit({
      owner: ownerB,
      tabId: tab.id,
    });
    useAgentArtifactFollowStore.getState().pin(ownerA, {
      kind: "tab",
      tabId: tab.id,
    });
    useAgentArtifactFollowStore.getState().pin(ownerB, {
      kind: "tab",
      tabId: tab.id,
    });
    expect(useAgentArtifactFollowStore.getState().records).toHaveLength(2);

    useWorkbenchStore.getState().closeTab(tab.id);

    expect(useWorkbenchStore.getState().tabs).toEqual([]);
    expect(useAgentArtifactAdmissionStore.getState().admissions).toEqual([]);
    expect(useAgentArtifactFollowStore.getState().records).toEqual([]);
  });

  it("forgets admissions and follow state for a resource evicted at capacity", async () => {
    const resourceTabs: WorkbenchTab[] = Array.from(
      { length: 32 },
      (_, index) => ({
        id: `file:${index}`,
        kind: "file" as const,
        title: `file-${index}.ts`,
        position: index + 1,
        pinned: false,
        dirty: false,
        resource: {
          rootId: "root-a",
          relativePath: `src/file-${index}.ts`,
        },
      }),
    );
    useWorkbenchStore.setState({
      project: {
        id: "project-a",
        name: "A",
        roots: [
          {
            id: "root-a",
            name: "A",
            ordinal: 0,
            availability: "available",
            message: null,
          },
        ],
      },
      tabs: [
        {
          id: "chat-a",
          kind: "chat",
          title: "Chat A",
          position: 0,
          pinned: false,
          dirty: false,
        },
        ...resourceTabs,
      ],
      activeTabId: "chat-a",
      flushActiveEditor: vi.fn(async () => undefined),
      loadEditor: vi.fn(async () => undefined),
      revealFile: vi.fn(async () => undefined),
    });
    useAgentArtifactAdmissionStore.getState().admit({
      owner: ownerA,
      tabId: "file:0",
    });
    useAgentArtifactAdmissionStore.getState().admit({
      owner: ownerB,
      tabId: "file:0",
    });
    useAgentArtifactFollowStore.getState().pin(ownerA, {
      kind: "tab",
      tabId: "file:0",
    });
    useAgentArtifactFollowStore.getState().pin(ownerB, {
      kind: "tab",
      tabId: "file:0",
    });
    expect(useAgentArtifactFollowStore.getState().records).toHaveLength(2);

    await useWorkbenchStore.getState().openFile({
      rootId: "root-a",
      relativePath: "src/new.ts",
    });

    expect(
      useWorkbenchStore.getState().tabs.some((tab) => tab.id === "file:0"),
    ).toBe(false);
    expect(useAgentArtifactAdmissionStore.getState().admissions).toEqual([]);
    expect(useAgentArtifactFollowStore.getState().records).toEqual([]);
  });
});

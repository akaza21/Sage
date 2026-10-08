import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { createElement, StrictMode } from "react";

import { clearAcpProjectionRegistry } from "../../../src/app/acpProjectionRegistry";
import { useWorkbenchStore } from "../../../src/app/store";
import { getWorkbenchPorts } from "../../../src/ports/workbenchRuntime";
import { acpDesktop } from "../../../src/services/acpDesktop";
import { AgentArtifactPane } from "../../../src/workbench/AgentArtifactPane";
import { AgentChatPanes } from "../../../src/workbench/AgentChatPanes";
import { Workbench } from "../../../src/workbench/Workbench";
import {
  applyBrowseIntent,
  browseSessionForOwner,
  clearBrowseSessions,
  harnessBrowseOwner,
  listBrowseSessions,
  setHarnessBrowseOwner,
  type HarnessBrowseOwner,
} from "../../../src/features/preview/harnessBrowse";

import {
  AGENT_VISUAL_SCENARIOS,
  EDITOR_VISUAL_SCENARIOS,
  PREVIEW_PROJECTION_FIXTURE_SCENARIOS,
  RECOVERY_VISUAL_SCENARIOS,
  RESOURCE_VISUAL_SCENARIOS,
  type VisualScenarioName,
} from "./visualScenarioTypes";
import { VISUAL_SCENARIOS, scenarioSurface } from "./visualScenarioCatalog";
import { buildAgentPages } from "./surfaces/agentVisualFixtures";
import { buildEditorPages } from "./surfaces/editorVisualFixtures";
import { buildRecoveryPages } from "./surfaces/recoveryVisualFixtures";
import { buildResourcePages } from "./surfaces/resourceVisualFixtures";
import {
  installVisualAgentArtifactFollowFixture,
  installVisualAgentPreviewIdleFixture,
  installVisualAgentResourceState,
} from "./surfaces/resourceVisualFixtures";
import {
  GENERATION,
  PROJECT_ID,
  RUNTIME_EPOCH,
  SESSION_ID,
} from "./visualFixtureIds";
import {
  installVisualDirtyEditor,
  installVisualEmptyEditorCanvas,
  installVisualEmptyEditorTools,
  installVisualExplorerActionFixture,
  installVisualExplorerScrollFixture,
  installVisualExplorerUnavailableActionFixture,
  installVisualRepositoryError,
  installVisualRepositoryLoading,
  installVisualRepositoryPrimaryState,
} from "./surfaces/editorVisualFixtures";
import * as barrel from "./visualScenarios";
import {
  buildVisualScenario,
  installVisualScenario,
} from "./visualScenarioShell";
import { fixtureTabs, visualSnapshotDiffRef } from "./visualDocumentFixtures";
import { applyVisualWorkbenchState } from "./visualWorkbenchState";

const initialState = useWorkbenchStore.getState();

function visualPreviewUrl(): string {
  const port = window.location.port || "1420";
  return `http://127.0.0.1:${port}/tests/ui/fixtures/preview.html`;
}

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
    return window.setTimeout(() => callback(performance.now()), 0);
  });
  vi.stubGlobal("cancelAnimationFrame", (handle: number) => {
    window.clearTimeout(handle);
  });
  Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
    configurable: true,
    value: vi.fn(),
  });
  vi.stubGlobal("matchMedia", () => ({
    matches: false,
    media: "",
    onchange: null,
    addEventListener: () => undefined,
    removeEventListener: () => undefined,
    addListener: () => undefined,
    removeListener: () => undefined,
    dispatchEvent: () => false,
  }));
});

afterEach(() => {
  cleanup();
  clearBrowseSessions();
  clearAcpProjectionRegistry();
  useWorkbenchStore.setState(initialState, true);
  Reflect.deleteProperty(HTMLElement.prototype, "scrollIntoView");
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

const SLICES = {
  agent: AGENT_VISUAL_SCENARIOS,
  editor: EDITOR_VISUAL_SCENARIOS,
  resource: [
    ...RESOURCE_VISUAL_SCENARIOS,
    ...PREVIEW_PROJECTION_FIXTURE_SCENARIOS,
  ],
  recovery: RECOVERY_VISUAL_SCENARIOS,
} as const;

const BUILDERS = {
  agent: buildAgentPages,
  editor: buildEditorPages,
  resource: buildResourcePages,
  recovery: buildRecoveryPages,
} as const;

describe("visual scenario surface ownership", () => {
  it("keeps the full catalogue explicit in its pre-split order", () => {
    expect(VISUAL_SCENARIOS.map((entry) => entry.name)).toEqual([
      "agent-single",
      "agent-dual",
      "agent-long",
      "agent-activity-family",
      "agent-declared-test",
      "agent-command-rollup",
      "agent-image",
      "agent-child-activity",
      "agent-picker-harness-model",
      "agent-picker-handoff",
      "agent-composer-narrow",
      "agent-composer-context-unknown",
      "agent-composer-context-known",
      "agent-composer-goal",
      "agent-composer-context-over-capacity",
      "agent-session-mode-closed",
      "agent-session-mode-open",
      "agent-session-mode-narrow",
      "agent-session-mode-forced-colors",
      "editor",
      "editor-picker-repository",
      "editor-picker-repository-filtered",
      "editor-picker-narrow",
      "editor-picker-zoom-200",
      "file",
      "diff",
      "preview",
      "agent-preview-starting",
      "agent-preview-health-failed",
      "agent-preview-exited",
      "error",
      "waiting",
    ]);
  });

  it("keeps the completed activity command as authored bounded output with stable history identity", () => {
    const activityPage = buildAgentPages("agent-activity-family")?.[0];
    const activityTurn = activityPage?.turns[0];
    const command = activityTurn?.items.find(
      (item) =>
        item.kind === "toolCall" &&
        item.payload.presentation.kind === "execute" &&
        item.payload.presentation.label === "Run focused checks",
    );

    expect(command).toMatchObject({
      id: "visual-turn-visual-chat-activity-family-1-item-3",
      kind: "toolCall",
      lifecycle: "completed",
      ordinal: 3,
      revision: "1",
      toolState: "completed",
      payload: {
        inputSummary: null,
        outputSummary:
          "Focused checks passed for ToolItem, TurnActivityItem, and AcpChatProjection. This deliberately bounded fixture output records the completed command result across parser coverage, mounted runtime lifecycle, visual projection ownership, accessibility labels, and compact-width containment so the activity card exercises the existing long-output overflow contract without using presentation text, input text, or raw process output.",
      },
    });
    expect(
      activityTurn?.items.map((item) => ({
        id: item.id,
        kind: item.kind,
        ordinal: item.ordinal,
        revision: item.revision,
      })),
    ).toEqual([
      {
        id: "visual-turn-visual-chat-activity-family-1-item-1",
        kind: "thoughtSummary",
        ordinal: 1,
        revision: "1",
      },
      {
        id: "visual-turn-visual-chat-activity-family-1-item-2",
        kind: "plan",
        ordinal: 2,
        revision: "1",
      },
      {
        id: "visual-turn-visual-chat-activity-family-1-item-3",
        kind: "toolCall",
        ordinal: 3,
        revision: "1",
      },
      {
        id: "visual-turn-visual-chat-activity-family-1-item-4",
        kind: "toolCall",
        ordinal: 4,
        revision: "1",
      },
      {
        id: "visual-turn-visual-chat-activity-family-1-item-5",
        kind: "toolCall",
        ordinal: 5,
        revision: "1",
      },
      {
        id: "visual-turn-visual-chat-activity-family-1-item-6",
        kind: "toolCall",
        ordinal: 6,
        revision: "1",
      },
      {
        id: "visual-turn-visual-chat-activity-family-1-item-7",
        kind: "toolCall",
        ordinal: 7,
        revision: "1",
      },
      {
        id: "visual-turn-visual-chat-activity-family-1-item-8",
        kind: "error",
        ordinal: 8,
        revision: "1",
      },
      {
        id: "visual-turn-visual-chat-activity-family-1-item-9",
        kind: "unsupported",
        ordinal: 9,
        revision: "1",
      },
    ]);
  });

  it("exposes a synchronous handoff invalidation action only for its scenario", () => {
    const visualWindow = window as typeof window & {
      __activateVisualHandoffTarget?: () => void;
    };
    const handoffScenario = buildVisualScenario("agent-picker-handoff");
    applyVisualWorkbenchState(handoffScenario, fixtureTabs(handoffScenario));

    expect(visualWindow.__activateVisualHandoffTarget).toBeTypeOf("function");
    visualWindow.__activateVisualHandoffTarget?.();
    expect(useWorkbenchStore.getState().activeTabId).toBe(
      "visual-chat-handoff-target",
    );
    expect(visualWindow.__activateVisualHandoffTarget).toBeUndefined();

    const editorScenario = buildVisualScenario("editor");
    applyVisualWorkbenchState(editorScenario, fixtureTabs(editorScenario));
    expect(visualWindow.__activateVisualHandoffTarget).toBeUndefined();
  });

  it("seeds the Editor repository fixture as one authoritative ready inventory", () => {
    const scenario = buildVisualScenario("editor");
    applyVisualWorkbenchState(scenario, fixtureTabs(scenario));

    expect(useWorkbenchStore.getState()).toMatchObject({
      repositoryInventoryPhase: "ready",
      repositoryStatusPhase: "ready",
      selectedRepository: { rootId: "visual-root", relativePath: "" },
      repositoryRefs: [{ rootId: "visual-root", relativePath: "" }],
    });
    expect(useWorkbenchStore.getState().selectedRepositoryStatus).toMatchObject(
      {
        rootId: "visual-root",
        state: "repository",
        branch: "feature/editor-polish",
      },
    );
  });

  it("assigns every scenario to exactly one surface with no duplicate names", () => {
    const owned = Object.values(SLICES).flat();
    expect(owned).toHaveLength(VISUAL_SCENARIOS.length);
    expect(new Set(owned).size).toBe(owned.length);
    expect([...owned].sort()).toEqual(
      VISUAL_SCENARIOS.map((entry) => entry.name).sort(),
    );
    const inOrder = [
      ...SLICES.agent,
      ...SLICES.editor,
      ...SLICES.resource,
      ...SLICES.recovery,
    ];
    expect(inOrder).toEqual(VISUAL_SCENARIOS.map((entry) => entry.name));
  });

  it("keeps the surface slices pairwise disjoint", () => {
    const slices = Object.values(SLICES);
    for (let outer = 0; outer < slices.length; outer += 1) {
      for (let inner = outer + 1; inner < slices.length; inner += 1) {
        expect(
          slices[outer].filter((name) => slices[inner].includes(name)),
        ).toEqual([]);
      }
    }
  });

  it("maps each scenario name to its owning surface", () => {
    const expected: Record<VisualScenarioName, keyof typeof SLICES> = {
      "agent-single": "agent",
      "agent-dual": "agent",
      "agent-long": "agent",
      "agent-activity-family": "agent",
      "agent-declared-test": "agent",
      "agent-command-rollup": "agent",
      "agent-image": "agent",
      "agent-child-activity": "agent",
      "agent-picker-harness-model": "agent",
      "agent-picker-handoff": "agent",
      "agent-composer-narrow": "agent",
      "agent-composer-context-unknown": "agent",
      "agent-composer-context-known": "agent",
      "agent-composer-goal": "agent",
      "agent-composer-context-over-capacity": "agent",
      "agent-session-mode-closed": "agent",
      "agent-session-mode-open": "agent",
      "agent-session-mode-narrow": "agent",
      "agent-session-mode-forced-colors": "agent",
      editor: "editor",
      "editor-picker-repository": "editor",
      "editor-picker-repository-filtered": "editor",
      "editor-picker-narrow": "editor",
      "editor-picker-zoom-200": "editor",
      file: "resource",
      diff: "resource",
      preview: "resource",
      "agent-preview-starting": "resource",
      "agent-preview-health-failed": "resource",
      "agent-preview-exited": "resource",
      error: "recovery",
      waiting: "recovery",
    };
    for (const scenario of VISUAL_SCENARIOS) {
      expect(scenarioSurface(scenario.name)).toBe(expected[scenario.name]);
    }
    expect(() =>
      scenarioSurface("unknown-scenario" as VisualScenarioName),
    ).toThrow("Unknown visual scenario");
  });

  it("lets exactly one surface builder claim each scenario", () => {
    for (const scenario of VISUAL_SCENARIOS) {
      const claimed = (
        Object.entries(BUILDERS) as Array<
          [keyof typeof BUILDERS, (typeof BUILDERS)[keyof typeof BUILDERS]]
        >
      ).filter(([, build]) => build(scenario.name) !== null);
      expect(claimed.map(([surface]) => surface)).toEqual([
        scenarioSurface(scenario.name),
      ]);
    }
  });

  it("keeps AV-01 phase fixture companions labelled separately from screenshot proof", () => {
    expect(PREVIEW_PROJECTION_FIXTURE_SCENARIOS).toEqual([
      "agent-preview-starting",
      "agent-preview-health-failed",
      "agent-preview-exited",
    ]);
    expect(
      PREVIEW_PROJECTION_FIXTURE_SCENARIOS.every(
        (name) => scenarioSurface(name) === "resource",
      ),
    ).toBe(true);
  });

  it("builds the same deterministic pages through the dispatcher and the owning builder", () => {
    for (const scenario of VISUAL_SCENARIOS) {
      const builder = BUILDERS[scenarioSurface(scenario.name)];
      expect(buildVisualScenario(scenario.name).pages).toEqual(
        builder(scenario.name),
      );
      expect(buildVisualScenario(scenario.name)).toEqual(
        buildVisualScenario(scenario.name),
      );
    }
  });

  it("keeps the compatibility barrel to its exact single-route allowlist", () => {
    expect(Object.keys(barrel).sort()).toEqual(["installVisualScenario"]);
    expect(typeof barrel.installVisualScenario).toBe("function");
  });

  it("keeps every surface installer resolving from its owning module", () => {
    for (const installer of [
      installVisualAgentResourceState,
      installVisualAgentArtifactFollowFixture,
      installVisualExplorerScrollFixture,
      installVisualExplorerActionFixture,
      installVisualExplorerUnavailableActionFixture,
      installVisualRepositoryError,
      installVisualRepositoryLoading,
      installVisualRepositoryPrimaryState,
      installVisualEmptyEditorTools,
      installVisualEmptyEditorCanvas,
      installVisualDirtyEditor,
    ]) {
      expect(typeof installer).toBe("function");
    }
  });

  it("seeds Preview with an admitted full owner and preserves two visible Chats", async () => {
    await installVisualScenario("agent-dual");
    const chats = useWorkbenchStore
      .getState()
      .tabs.filter((tab) => tab.kind === "chat");
    const first = chats[0];
    const second = chats[1];
    if (!first || !second)
      throw new Error("Visual fixture requires two Chats.");
    const firstOwner = harnessBrowseOwner();
    expect(firstOwner).toEqual({
      projectId: PROJECT_ID,
      projectGeneration: GENERATION,
      transitionEpoch: 0,
      chatId: first.id,
      sessionId: SESSION_ID,
      runtimeEpoch: RUNTIME_EPOCH,
    });
    if (!firstOwner) throw new Error("Visual browse owner was absent.");
    expect(
      applyBrowseIntent({
        owner: firstOwner,
        url: visualPreviewUrl(),
      }),
    ).not.toBeNull();
    const secondOwner: HarnessBrowseOwner = {
      ...firstOwner,
      chatId: second.id as HarnessBrowseOwner["chatId"],
    };
    setHarnessBrowseOwner(secondOwner, [first.id, second.id]);
    expect(
      applyBrowseIntent({
        owner: secondOwner,
        url: `${visualPreviewUrl()}?chat=2`,
      }),
    ).not.toBeNull();
    expect(browseSessionForOwner(firstOwner)?.href).toContain("preview.html");
    expect(browseSessionForOwner(secondOwner)?.href).toContain("chat=2");
  });

  it("does not self-seed Preview ownership before the Chat shell mounts", async () => {
    const scenario = buildVisualScenario("preview");
    const chat = scenario.pages[0];
    if (!chat) throw new Error("Preview fixture requires a Chat.");
    await expect(installVisualScenario("preview")).rejects.toThrow(
      "Visual Preview fixture owner was not established.",
    );
    expect(harnessBrowseOwner()).toBeNull();
    const expectedOwner = {
      projectId: PROJECT_ID,
      projectGeneration: GENERATION,
      transitionEpoch: 0,
      chatId: chat.chatId,
      sessionId: SESSION_ID,
      runtimeEpoch: RUNTIME_EPOCH,
    };
    expect(browseSessionForOwner(expectedOwner)).toBeNull();
  });

  it("rejects a bounded Preview wait when the mounted owner has the wrong tuple", async () => {
    const scenario = buildVisualScenario("preview");
    const chat = scenario.pages[0];
    if (!chat) throw new Error("Preview fixture requires a Chat.");
    const installing = installVisualScenario("preview");
    await new Promise<void>((resolve) =>
      requestAnimationFrame(() => resolve()),
    );
    const wrongOwner: HarnessBrowseOwner = {
      projectId: PROJECT_ID,
      projectGeneration: GENERATION,
      transitionEpoch: 0,
      chatId: chat.chatId,
      sessionId: "wrong-session",
      runtimeEpoch: RUNTIME_EPOCH,
    };
    setHarnessBrowseOwner(wrongOwner, [chat.chatId]);
    await expect(installing).rejects.toThrow(
      "Visual Preview fixture owner was not established.",
    );
    expect(browseSessionForOwner(wrongOwner)).toBeNull();
  });

  it("keeps the Preview fixture owner and selected tab through mounted Chat handoff", async () => {
    const view = render(
      createElement(
        "div",
        null,
        createElement(AgentChatPanes),
        createElement(AgentArtifactPane),
      ),
    );
    await installVisualScenario("preview");
    await waitFor(() =>
      expect(harnessBrowseOwner()).toMatchObject({
        projectId: PROJECT_ID,
        projectGeneration: GENERATION,
        transitionEpoch: 0,
        sessionId: SESSION_ID,
        runtimeEpoch: RUNTIME_EPOCH,
      }),
    );
    await waitFor(() =>
      expect(view.getByRole("tab", { name: "Preview" })).toHaveAttribute(
        "aria-selected",
        "true",
      ),
    );
    await waitFor(() =>
      expect(view.getByTitle("Sage browser")).toBeInTheDocument(),
    );
  });

  it("waits through the StrictMode lazy Chat bootstrap before admitting Preview browse", async () => {
    localStorage.setItem(
      "sage.workbench.layout.v1",
      JSON.stringify({
        editorToolRailWidth: 264,
        agentChatSplitPercent: 50,
        windowMode: "agent",
      }),
    );
    const installing = installVisualScenario("preview");
    await act(async () => {
      await new Promise<void>((resolve) =>
        requestAnimationFrame(() => resolve()),
      );
    });
    expect(harnessBrowseOwner()).toBeNull();
    const view = render(
      createElement(StrictMode, null, createElement(Workbench)),
    );
    expect(view.container.querySelector(".agent-chat-panes")).toBeNull();
    await act(async () => {
      await vi.dynamicImportSettled();
      // Let the mounted Chat subscription publish its ownership signal in the
      // same controlled render turn; do not depend on a load-sensitive poll.
      await new Promise<void>((resolve) =>
        requestAnimationFrame(() => resolve()),
      );
    });
    await waitFor(() =>
      expect(view.container.querySelector(".agent-chat-panes")).not.toBeNull(),
    );
    await waitFor(() =>
      expect(harnessBrowseOwner()).toEqual({
        projectId: PROJECT_ID,
        projectGeneration: GENERATION,
        transitionEpoch: 0,
        chatId: "visual-chat-preview",
        sessionId: SESSION_ID,
        runtimeEpoch: RUNTIME_EPOCH,
      }),
    );
    await installing;
    await waitFor(() =>
      expect(view.getByRole("tab", { name: "Preview" })).toHaveAttribute(
        "aria-selected",
        "true",
      ),
    );
    await waitFor(() =>
      expect(view.getByTitle("Sage browser")).toBeInTheDocument(),
    );
    const owner = harnessBrowseOwner();
    if (!owner) throw new Error("Preview fixture browse owner was absent.");
    expect(browseSessionForOwner(owner)?.href).toBe(visualPreviewUrl());
    expect(listBrowseSessions()).toHaveLength(1);
  });

  it("removes Preview after the mounted fixture clears browsing", async () => {
    const view = render(
      createElement(
        "div",
        null,
        createElement(AgentChatPanes),
        createElement(AgentArtifactPane),
      ),
    );
    await installVisualScenario("preview");
    await installVisualAgentPreviewIdleFixture();
    await waitFor(() =>
      expect(view.queryByRole("tab", { name: "Preview" })).toBeNull(),
    );
    expect(view.queryByTitle("Sage browser")).toBeNull();
  });

  it("runs the diff fixture card through the real snapshot loader without Git", async () => {
    render(
      createElement(
        "div",
        null,
        createElement(AgentChatPanes),
        createElement(AgentArtifactPane),
      ),
    );
    await installVisualScenario("diff");
    const snapshotBridge = acpDesktop.getFileSnapshot;
    const snapshot = vi
      .spyOn(acpDesktop, "getFileSnapshot")
      .mockImplementation(snapshotBridge);
    const git = vi.spyOn(getWorkbenchPorts().repository, "getFileDiff");

    expect(
      await screen.findByRole(
        "region",
        { name: "Files edited in turn 1" },
        { timeout: 5_000 },
      ),
    ).toHaveTextContent("1 file changed");
    fireEvent.click(screen.getByRole("button", { name: /file changed/i }));
    fireEvent.click(
      await screen.findByRole("button", {
        name: "Open diff Sage / src/workbench/AgentChatPanes.tsx",
      }),
    );

    expect(
      await screen.findByRole("table", { name: "Unified file diff" }),
    ).toBeVisible();
    expect(
      screen.getByText(
        "-const visibleChats = useVisibleChats({ maximum: 1 });",
      ),
    ).toBeVisible();
    expect(
      screen.getByText(
        "+const visibleChats = useVisibleChats({ maximum: 2 });",
      ),
    ).toBeVisible();
    expect(screen.getByText("Saved agent change")).toBeVisible();
    expect(snapshot).toHaveBeenCalledOnce();
    expect(snapshot).toHaveBeenCalledWith({
      ...visualSnapshotDiffRef,
      projectGeneration: GENERATION,
      expectedProjectionRevision: "42",
    });
    expect(git).not.toHaveBeenCalled();
  });

  it("seeds the resource follow fixture's second Chat browse owner before intent", async () => {
    await installVisualScenario("agent-dual");
    const chats = useWorkbenchStore
      .getState()
      .tabs.filter((tab) => tab.kind === "chat");
    const second = chats[1];
    if (!second) throw new Error("Visual fixture requires a second Chat.");
    await installVisualAgentArtifactFollowFixture();
    const owner = harnessBrowseOwner();
    expect(owner).toEqual({
      projectId: PROJECT_ID,
      projectGeneration: GENERATION,
      transitionEpoch: 0,
      chatId: second.id,
      sessionId: SESSION_ID,
      runtimeEpoch: RUNTIME_EPOCH,
    });
    if (!owner) throw new Error("Follow fixture browse owner was absent.");
    expect(browseSessionForOwner(owner)?.href).toBe(visualPreviewUrl());
  });
});

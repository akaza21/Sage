import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { startTransition, useLayoutEffect } from "react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { PreviewProjectionChangedEvent } from "../domain/previewProjection";

const previewBridge = vi.hoisted(() => ({
  discover: vi.fn(async () => ({ targets: [], truncated: false })),
  health: vi.fn(async () => ({ healthy: true, origin: null })),
  start: vi.fn(async () => undefined),
  stop: vi.fn(async () => undefined),
  subscribe: vi.fn(
    async (_callback: (event: PreviewProjectionChangedEvent) => void) => () =>
      undefined,
  ),
}));

vi.mock("../features/preview/previewDesktop", () => ({
  discoverPreviewTargets: previewBridge.discover,
  getPreviewHealth: previewBridge.health,
  previewDesktopAvailable: () => true,
  startPreviewTarget: previewBridge.start,
  stopPreviewTarget: previewBridge.stop,
}));
vi.mock("../services/acpDesktop", () => ({
  acpDesktop: { subscribePreviewProjectionChanges: previewBridge.subscribe },
}));
// The pane host keeps its real owner contract: pin on mount, release on hide.
vi.mock("./AgentChatPanes", async () => {
  const owner = await import("../app/agentChatOwner");
  return {
    AgentChatPanes: () => {
      useLayoutEffect(() => {
        owner.pinAgentChatOwner("chat-focused");
        return owner.releaseAgentChatOwner;
      }, []);
      return null;
    },
  };
});
vi.mock("./LeftSidebar", () => ({ LeftSidebar: () => null }));
vi.mock("./ActiveEditorCanvas", () => ({ default: () => null }));
vi.mock("./DirtyTabDialog", () => ({ DirtyTabDialog: () => null }));
vi.mock("./RightSidebar", () => ({ RightSidebar: () => null }));
vi.mock("./StatusBar", () => ({ StatusBar: () => null }));
vi.mock("./TabStrip", () => ({ TabStrip: () => null }));
vi.mock("./useCompactWorkbenchLayout", () => ({
  useCompactWorkbenchLayout: () => false,
}));

import {
  acpProjectionStoreFor,
  clearAcpProjectionRegistry,
  pinAcpProjectionIds,
} from "../app/acpProjectionRegistry";
import { clearAgentChatOwner } from "../app/agentChatOwner";
import { useWorkbenchStore } from "../app/store";
import type {
  AcpChatPage,
  ProjectGeneration,
  SageChatId,
  SageProjectId,
} from "../domain/acp";
import {
  applyBrowseIntent,
  clearBrowseSessions,
  setHarnessBrowseOwner,
  type HarnessBrowseOwner,
} from "../features/preview/harnessBrowse";
import { useAgentSettingsState } from "../features/settings/agentSettingsState";
import { settleLazyModules } from "../test/settleLazyModules";
import { Workbench } from "./Workbench";
import { writeWorkbenchLayout } from "./workbenchLayoutStorage";

const initialState = useWorkbenchStore.getState();
const browseOwner = {
  projectId: "project-a",
  projectGeneration: "0",
  transitionEpoch: 0,
  chatId: "chat-focused",
  sessionId: "session-a",
  runtimeEpoch: "3",
} as HarnessBrowseOwner;

beforeEach(() => {
  writeWorkbenchLayout({ windowMode: "agent" });
  vi.stubGlobal("innerWidth", 1600);
  pinAcpProjectionIds(["chat-focused"]);
  const scope = {
    projectId: "project-a" as SageProjectId,
    projectGeneration: "0" as ProjectGeneration,
    chatId: "chat-focused" as SageChatId,
  };
  const store = acpProjectionStoreFor("chat-focused");
  store.getState().activateScope(scope);
  store.getState().applyAuthoritativePage(scope, {
    projectId: "project-a",
    chatId: "chat-focused",
    title: "Focused",
    projectionRevision: "1",
    currentSessionId: "session-a",
    sessions: [{ id: "session-a", ordinal: 1, runtimeEpoch: "3" }],
    turns: [],
    nextCursor: null,
  } as unknown as AcpChatPage);
  setHarnessBrowseOwner(browseOwner, ["chat-focused"]);
  applyBrowseIntent({ owner: browseOwner, url: "http://127.0.0.1:5173/" });
  useWorkbenchStore.setState({
    project: { id: "project-a", name: "Project A", roots: [] },
    projectGeneration: 0,
    projectTransitioning: false,
    tabs: [
      {
        id: "chat-focused",
        kind: "chat",
        title: "Focused",
        position: 0,
        pinned: false,
        dirty: false,
      },
    ],
    activeTabId: "chat-focused",
  });
});

afterEach(() => {
  cleanup();
  useAgentSettingsState.getState().close();
  clearBrowseSessions();
  clearAcpProjectionRegistry();
  clearAgentChatOwner();
  vi.unstubAllGlobals();
  useWorkbenchStore.setState(initialState, true);
  vi.clearAllMocks();
});

async function openPreview() {
  render(<Workbench />);
  await settleLazyModules();
  await act(async () => {
    previewBridge.subscribe.mock.calls.at(-1)?.[0]({
      kind: "preview",
      preview: {
        ...browseOwner,
        previewRevision: "1" as never,
        phase: "active",
        failure: null,
        origin: "http://127.0.0.1:5173/",
        target: {
          ...browseOwner,
          expectedTransitionEpoch: 0,
          rootId: "root-a" as never,
          scriptName: "dev",
        },
      },
    });
  });
  const frame = screen.getByTitle("Sage browser");
  expect(frame).toBeVisible();
  return frame;
}

const modeRadio = (name: "Agent" | "Editor") =>
  screen.getByRole("radio", { name });

async function resizeWhileHidden() {
  act(() => {
    vi.stubGlobal("innerWidth", 1280);
    window.dispatchEvent(new Event("resize"));
  });
  await settleLazyModules();
}

it("keeps the Preview frame asleep behind Settings through a resize", async () => {
  const frame = await openPreview();

  fireEvent.keyDown(window, { key: ",", ctrlKey: true });
  await settleLazyModules();
  expect(screen.getByRole("region", { name: "Agent settings" })).toBeVisible();
  expect(frame.isConnected).toBe(true);
  expect(frame).not.toBeVisible();
  expect(previewBridge.stop).not.toHaveBeenCalled();
  await resizeWhileHidden();
  fireEvent.click(screen.getByRole("button", { name: "Close settings" }));
  await settleLazyModules();

  expect(screen.getByTitle("Sage browser")).toBe(frame);
  expect(frame).toBeVisible();
});

it("keeps the Preview frame when the layout changes while Editor is shown", async () => {
  const frame = await openPreview();

  fireEvent.click(modeRadio("Editor"));
  await settleLazyModules();
  expect(frame.isConnected).toBe(true);
  expect(previewBridge.stop).not.toHaveBeenCalled();
  await resizeWhileHidden();
  fireEvent.click(modeRadio("Agent"));
  await settleLazyModules();

  expect(screen.getByTitle("Sage browser")).toBe(frame);
  expect(frame).toBeVisible();
});

it("keeps the Preview frame across transition mode switches with a layout change", async () => {
  const frame = await openPreview();

  await act(async () => {
    startTransition(() => {
      fireEvent.click(modeRadio("Editor"));
    });
  });
  await settleLazyModules();
  expect(previewBridge.stop).not.toHaveBeenCalled();
  await resizeWhileHidden();
  await act(async () => {
    startTransition(() => {
      fireEvent.click(modeRadio("Agent"));
    });
  });
  await settleLazyModules();

  expect(screen.getByTitle("Sage browser")).toBe(frame);
  expect(frame).toBeVisible();
});

it.each([false, true])(
  "restores Editor after Browser settings close (site permissions: %s)",
  async (site) => {
    writeWorkbenchLayout({ windowMode: "editor" });
    render(<Workbench />);
    await settleLazyModules();
    act(() =>
      useAgentSettingsState.getState().open("Browser", "browser-a", site),
    );
    await settleLazyModules();
    expect(
      screen.getByRole("region", { name: "Agent settings" }),
    ).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Close settings" }));
    expect(modeRadio("Editor")).toBeChecked();
  },
);

import { beforeEach, expect, it, vi } from "vitest";
import type * as InheritedComposerModel from "../features/chat/inheritedComposerModel";

const fake = vi.hoisted(() => {
  const state = {
    project: { id: "project" },
    projectGeneration: 0,
    nativeTransitionEpoch: 0,
    chatOwnershipEpoch: 0,
    projectTransitioning: false,
    tabs: [{ id: "parent", kind: "chat", title: "Parent" }],
    activeTabId: "parent",
    selectedHarnessId: "custom-acp",
    chatUiById: {},
    harnesses: [
      { id: "custom-acp", transport: "acp", bindingId: "binding-custom" },
    ],
    captureChatOwnership: vi.fn((chatId: string) => ({ chatId })),
    setChatHarnessSelection: vi.fn(() => true),
    attachChatContext: vi.fn(() => true),
  };
  const parentPage = {
    currentSessionId: "parent-session",
    sessions: [
      {
        id: "parent-session",
        bindingId: "binding-custom",
        modelId: "model-selected",
      },
    ],
    sessionConfig: { sageAgentSessionId: "parent-session", configOptions: [] },
  };
  const sidePage = { projectionRevision: "2" };
  const projection = {
    activateScope: vi.fn(),
    applyAuthoritativePage: vi.fn(),
  };
  return {
    state,
    parentPage,
    sidePage,
    projection,
    native: vi.fn(),
    bootstrap: vi.fn(),
    start: vi.fn(),
    inherit: vi.fn(),
    attach: state.attachChatContext,
  };
});
vi.mock("../app/store", () => ({
  useWorkbenchStore: {
    getState: () => fake.state,
    setState: (patch: Partial<typeof fake.state>) =>
      Object.assign(fake.state, patch),
  },
}));
vi.mock("../ports/nativeInvoke", () => ({
  isTauriRuntime: () => true,
  invokeNative: fake.native,
}));
vi.mock("../ports/workbenchRuntime", () => ({
  getWorkbenchPorts: () => ({
    project: {
      getBootstrapSnapshot: fake.bootstrap,
    },
  }),
}));
vi.mock("../app/projectProjection", () => ({
  currentProjectLayout: () => ({
    projectId: fake.state.project.id,
    tabs: fake.state.tabs,
    activeTabId: fake.state.activeTabId,
  }),
}));
vi.mock("../app/chatTransitions", () => ({
  createChatTransitionContext: () => ({
    applyProjection: async (snapshot: {
      tabs: typeof fake.state.tabs;
      activeTabId: string;
      transitionEpoch: number;
    }) => {
      fake.state.tabs = snapshot.tabs;
      fake.state.activeTabId = snapshot.activeTabId;
      fake.state.nativeTransitionEpoch = snapshot.transitionEpoch;
    },
  }),
}));
vi.mock("../app/acpProjectionRegistry", () => ({
  acpProjectionStoreFor: (id: string) => ({
    getState: () =>
      id === "parent" ? { page: fake.parentPage } : fake.projection,
  }),
}));
vi.mock("../features/chat/inheritedComposerModel", async (importOriginal) => ({
  ...(await importOriginal<typeof InheritedComposerModel>()),
  inheritComposerModel: fake.inherit,
}));
vi.mock("../services/acpDesktop", () => ({
  acpDesktop: {
    getChatPage: async () => fake.sidePage,
    startSession: fake.start,
  },
}));

import {
  cancelCloseSideChat,
  closeSideChat,
  openSideChat,
  requestCloseSideChat,
} from "./sideChatActions";
import { useSideChatState } from "./sideChatState";

const quote = {
  turnId: "turn",
  itemId: "answer",
  text: "quoted",
  startOffset: 0,
  endOffset: 6,
};
const sideTab = { id: "side-test", kind: "chat", title: "Side chat" };
function snapshot(open: boolean, epoch = 1) {
  return {
    tabs: open ? [fake.state.tabs[0], sideTab] : [fake.state.tabs[0]],
    activeTabId: open ? sideTab.id : "parent",
    transitionEpoch: epoch,
  };
}
beforeEach(() => {
  vi.clearAllMocks();
  Object.assign(fake.state, {
    projectGeneration: 0,
    nativeTransitionEpoch: 0,
    chatOwnershipEpoch: 0,
    projectTransitioning: false,
    activeTabId: "parent",
    tabs: [{ id: "parent", kind: "chat", title: "Parent" }],
  });
  useSideChatState.setState({ sides: [], pendingClose: null });
  cancelCloseSideChat();
  fake.start.mockResolvedValue({ projection: fake.sidePage });
  fake.native.mockImplementation(async (command: string) => {
    if (command === "get_confirm_close_side_chat") return true;
    const next = snapshot(
      command === "open_side_chat",
      fake.state.nativeTransitionEpoch + 1,
    );
    return {
      committed: true,
      transitionEpoch: next.transitionEpoch,
      snapshot: next,
      recoveryError: null,
    };
  });
});

it("creates a temporary Chat on the parent's harness and model, seeded with the selection, without fork or an eager session", async () => {
  await openSideChat("parent", quote);
  expect(fake.native).toHaveBeenCalledWith(
    "open_side_chat",
    expect.objectContaining({ parentChatId: "parent" }),
  );
  expect(fake.state.setChatHarnessSelection).toHaveBeenCalledWith(
    { chatId: "side-test" },
    { harnessId: "custom-acp", modelId: "model-selected" },
  );
  // The first send starts the session exactly like any new Chat.
  expect(fake.start).not.toHaveBeenCalled();
  expect(fake.attach).toHaveBeenCalledWith(
    { chatId: "side-test" },
    expect.objectContaining({
      kind: "transcriptQuote",
      sourceItemId: "answer",
      text: "quoted",
    }),
  );
  expect(fake.state.projectTransitioning).toBe(false);
  expect(useSideChatState.getState().sides[0].parentChatId).toBe("parent");
});

it("labels the side composer only with the model it actually starts on (Codex s2/f0244)", async () => {
  await openSideChat("parent", quote);
  expect(fake.inherit).toHaveBeenCalledWith(
    "side-test",
    fake.parentPage.sessionConfig,
    { startsOnModelId: "model-selected" },
  );
});

it("tells the label a side chat starts on no model when the parent's session has none", async () => {
  const session = fake.parentPage.sessions[0] as { modelId: string | null };
  const prior = session.modelId;
  session.modelId = null;
  try {
    await openSideChat("parent", quote);
  } finally {
    session.modelId = prior;
  }
  expect(fake.inherit).toHaveBeenCalledWith(
    "side-test",
    fake.parentPage.sessionConfig,
    { startsOnModelId: null },
  );
});

it("starts the side chat on the wire-confirmed model rather than a stale request (R3-02)", async () => {
  Object.assign(fake.parentPage.sessionConfig, {
    confirmedModel: { id: "wire-model", name: "Synthetic replay" },
  });
  try {
    await openSideChat("parent", quote);
    expect(fake.state.setChatHarnessSelection).toHaveBeenCalledWith(
      { chatId: "side-test" },
      { harnessId: "custom-acp", modelId: "wire-model" },
    );
    expect(fake.inherit).toHaveBeenCalledWith(
      "side-test",
      fake.parentPage.sessionConfig,
      { startsOnModelId: "wire-model" },
    );
  } finally {
    Reflect.deleteProperty(fake.parentPage.sessionConfig, "confirmedModel");
  }
});

it("asks before closing and passes the opt-out to Rust only on confirmation", async () => {
  await openSideChat("parent", quote);
  await requestCloseSideChat("side-test");
  expect(useSideChatState.getState().pendingClose).toBe("side-test");
  expect(
    fake.native.mock.calls.filter(([command]) => command === "close_side_chat"),
  ).toHaveLength(0);
  cancelCloseSideChat();
  expect(useSideChatState.getState().sides).toHaveLength(1);
  await closeSideChat("side-test", true);
  expect(fake.native).toHaveBeenCalledWith(
    "close_side_chat",
    expect.objectContaining({ dontAskAgain: true }),
  );
  expect(useSideChatState.getState().sides).toHaveLength(0);
});

it("uses the persisted native opt-out on subsequent closes", async () => {
  await openSideChat("parent", quote);
  fake.native.mockImplementation(async (command: string) => {
    if (command === "get_confirm_close_side_chat") return false;
    return {
      committed: true,
      transitionEpoch: 2,
      snapshot: snapshot(false, 2),
      recoveryError: null,
    };
  });
  await requestCloseSideChat("side-test");
  expect(useSideChatState.getState().pendingClose).toBeNull();
  expect(useSideChatState.getState().sides).toHaveLength(0);
});

it("reconciles an ambiguous native creation without replaying it", async () => {
  fake.native.mockRejectedValueOnce(new Error("response lost"));
  fake.bootstrap.mockResolvedValue(snapshot(true));
  await openSideChat("parent", quote);
  expect(fake.native).toHaveBeenCalledTimes(1);
  expect(useSideChatState.getState().sides[0].id).toBe("side-test");
});

it("rethrows an IPC failure that did not commit and releases the Project lock", async () => {
  fake.native.mockRejectedValueOnce(new Error("response lost"));
  fake.bootstrap.mockResolvedValue(snapshot(false, 0));
  await expect(openSideChat("parent", quote)).rejects.toThrow("response lost");
  expect(useSideChatState.getState().sides).toHaveLength(0);
  expect(fake.state.projectTransitioning).toBe(false);
});

it("replaces the Project's existing side chat only after its close is confirmed", async () => {
  await openSideChat("parent", quote);
  await openSideChat("parent", { ...quote, text: "second" });
  expect(useSideChatState.getState().pendingClose).toBe("side-test");
  expect(fake.attach).toHaveBeenCalledTimes(1);
  await closeSideChat("side-test", false);
  expect(fake.attach).toHaveBeenLastCalledWith(
    { chatId: "side-test" },
    expect.objectContaining({ text: "second" }),
  );
  expect(useSideChatState.getState().sides).toHaveLength(1);
});

it("reports a selection that could not be attached and releases the Project lock", async () => {
  fake.attach.mockReturnValueOnce(false);
  await expect(openSideChat("parent", quote)).rejects.toThrow(
    "could not be attached",
  );
  expect(fake.start).not.toHaveBeenCalled();
  expect(fake.state.projectTransitioning).toBe(false);
});

it("forgets a pending replacement when the close fails, so a later close never reopens it", async () => {
  await openSideChat("parent", quote);
  await openSideChat("parent", { ...quote, text: "second" });
  const succeed = fake.native.getMockImplementation()!;
  fake.native.mockImplementation(async (command: string, args: unknown) => {
    if (command === "close_side_chat") throw new Error("close refused");
    return succeed(command, args);
  });
  fake.bootstrap.mockResolvedValue(snapshot(true, 1));
  await expect(closeSideChat("side-test", false)).rejects.toThrow(
    "close refused",
  );
  fake.native.mockImplementation(succeed);
  await closeSideChat("side-test", false);
  expect(fake.attach).toHaveBeenCalledTimes(1);
  expect(useSideChatState.getState().sides).toHaveLength(0);
});

it("forgets a pending replacement when asking to close fails", async () => {
  await openSideChat("parent", quote);
  const succeed = fake.native.getMockImplementation()!;
  fake.native.mockImplementation(async (command: string, args: unknown) => {
    if (command === "get_confirm_close_side_chat")
      throw new Error("preference unavailable");
    return succeed(command, args);
  });
  await expect(
    openSideChat("parent", { ...quote, text: "second" }),
  ).rejects.toThrow("preference unavailable");
  fake.native.mockImplementation(succeed);
  await closeSideChat("side-test", false);
  expect(fake.attach).toHaveBeenCalledTimes(1);
  expect(useSideChatState.getState().sides).toHaveLength(0);
});

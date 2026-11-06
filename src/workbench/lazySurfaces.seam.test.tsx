import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { createElement } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { clearAcpProjectionRegistry } from "../app/acpProjectionRegistry";
import {
  seedSnapshotOwner,
  snapshotPage,
  snapshotRef,
  snapshotResource,
} from "../app/agentDiff.fixture";
import { pinAgentChatOwner } from "../app/agentChatOwner";
import { openAgentArtifact } from "../app/openAgentArtifact";
import { useWorkbenchStore } from "../app/store";
import type * as NativeInvoke from "../ports/nativeInvoke";
import type * as UserTerminalDesktop from "../services/userTerminalDesktop";
import { FileMarkdownPreview } from "../features/files/FileMarkdownPreview";
import { acpDesktop } from "../services/acpDesktop";
import { settleLazyModules } from "../test/settleLazyModules";
import { registerWorkbenchSurfaces } from "./registeredSurfaces";
import { renderFileOrDiffCanvas } from "./surfaceRegistry";
import { Workbench } from "./Workbench";
import { writeWorkbenchLayout } from "./workbenchLayoutStorage";

// Each named surface records when its module is first evaluated. A static
// import anywhere on the startup path would evaluate it before the Workbench
// renders; a dynamic import evaluates it only when the surface opens.
const evaluated = vi.hoisted(() => new Set<string>());

vi.mock("./SettingsSurface", async (importOriginal) => {
  evaluated.add("SettingsSurface");
  return importOriginal();
});
vi.mock("./AgentWorkbenchBody", async (importOriginal) => {
  evaluated.add("AgentWorkbenchBody");
  return importOriginal();
});
vi.mock("./AgentArtifactPane", async (importOriginal) => {
  evaluated.add("AgentArtifactPane");
  return importOriginal();
});
vi.mock("./AgentChatPanes", async (importOriginal) => {
  evaluated.add("AgentChatPanes");
  return importOriginal();
});
vi.mock("./LeftSidebar", async (importOriginal) => {
  evaluated.add("LeftSidebar");
  return importOriginal();
});
vi.mock("./TerminalDockShell", async (importOriginal) => {
  evaluated.add("TerminalDockShell");
  return importOriginal();
});
// The dock's xterm canvas needs a real browser, so it is replaced; its
// module is still loaded through the shell's own dynamic import.
vi.mock("./TerminalDock", () => {
  evaluated.add("TerminalDock");
  return {
    default: ({ session }: { session: { sessionId: string } }) =>
      createElement("output", null, `Shell ${session.sessionId}`),
  };
});
const native = vi.hoisted(() => ({
  tauri: false,
  terminalScopes: [] as unknown[],
}));
vi.mock("../ports/nativeInvoke", async (importOriginal) => ({
  ...(await importOriginal<typeof NativeInvoke>()),
  isTauriRuntime: () => native.tauri,
}));
vi.mock("../services/userTerminalDesktop", async (importOriginal) => ({
  ...(await importOriginal<typeof UserTerminalDesktop>()),
  createUserTerminal: async (scope: unknown) => {
    native.terminalScopes.push(scope);
    return {
      sessionId: "f99cd56a-32d9-4600-a1f6-000000000001",
      revision: "1",
      shellPath: "/bin/sh",
    };
  },
  closeUserTerminal: async () => undefined,
}));
// The dock's shown/hidden preference is stored natively; start it hidden.
vi.mock("../services/terminalDockSettingsDesktop", () => ({
  getTerminalDockVisible: async () => false,
  setTerminalDockVisible: async (visible: boolean) => visible,
}));
vi.mock("./SourceControlView", async (importOriginal) => {
  evaluated.add("SourceControlView");
  return importOriginal();
});
vi.mock("../features/search/SearchSurface", async (importOriginal) => {
  evaluated.add("SearchSurface");
  return importOriginal();
});
vi.mock("../features/preview/PreviewSurface", async (importOriginal) => {
  evaluated.add("PreviewSurface");
  return importOriginal();
});
vi.mock(
  "../features/preview/AgentPreviewPresentation",
  async (importOriginal) => {
    evaluated.add("AgentPreviewPresentation");
    return importOriginal();
  },
);
vi.mock("../features/diff/DiffSurface", async (importOriginal) => {
  evaluated.add("DiffSurface");
  return importOriginal();
});
vi.mock("../features/files/FileSurface", async (importOriginal) => {
  evaluated.add("FileSurface");
  return importOriginal();
});
vi.mock("../features/browser/BrowserSurface", async (importOriginal) => {
  evaluated.add("BrowserSurface");
  return importOriginal();
});
vi.mock("../features/chat/acp/MermaidFigure", async (importOriginal) => {
  evaluated.add("MermaidFigure");
  return importOriginal();
});
vi.mock("./useCompactWorkbenchLayout", () => ({
  useCompactWorkbenchLayout: () => false,
}));
vi.mock("../features/chat/ChatSurface", () => ({
  ChatSurface: () =>
    createElement("section", { "aria-label": "Agent chat" }, "Active chat"),
}));

const initialState = useWorkbenchStore.getState();

// Each case starts in the Editor; the Agent case chooses Agent itself.
beforeEach(() => writeWorkbenchLayout({ windowMode: "editor" }));

afterEach(() => {
  cleanup();
  native.tauri = false;
  native.terminalScopes = [];
  vi.restoreAllMocks();
  localStorage.clear();
  clearAcpProjectionRegistry();
  pinAgentChatOwner(null);
  useWorkbenchStore.setState(initialState, true);
});

describe("Q04 lazy surfaces", () => {
  // Runs first: the module graph is shared by the whole file.
  it("starts the Editor with every named surface still unloaded", async () => {
    render(createElement(Workbench));
    await settleLazyModules();

    expect(
      screen.getByRole("complementary", { name: "Project tools" }),
    ).toBeInTheDocument();
    expect([...evaluated]).toEqual([]);
  });

  it("loads Settings only when it is opened and shows it", async () => {
    render(createElement(Workbench));
    await settleLazyModules();
    expect(evaluated.has("SettingsSurface")).toBe(false);

    fireEvent.click(screen.getByRole("button", { name: "Open settings" }));
    await settleLazyModules();

    expect(evaluated.has("SettingsSurface")).toBe(true);
    const settings = await screen.findByRole("region", { name: "Settings" });
    expect(
      within(settings).getByRole("tab", { name: "Editor" }),
    ).toBeInTheDocument();
  });

  it("loads Search and Source Control only when their tools are opened", async () => {
    useWorkbenchStore.setState({ sidebarOpen: true, sidebarView: "explorer" });
    render(createElement(Workbench));
    await settleLazyModules();
    expect(evaluated.has("SearchSurface")).toBe(false);
    expect(evaluated.has("SourceControlView")).toBe(false);

    fireEvent.click(screen.getByRole("button", { name: "Search" }));
    await settleLazyModules();
    expect(evaluated.has("SearchSurface")).toBe(true);
    expect(
      await screen.findByRole("searchbox", { name: "Search text" }),
    ).toBeInTheDocument();
    expect(evaluated.has("SourceControlView")).toBe(false);

    fireEvent.click(screen.getByRole("button", { name: "Source Control" }));
    await settleLazyModules();
    expect(evaluated.has("SourceControlView")).toBe(true);
    expect(
      await screen.findByRole("button", { name: "Refresh Source Control" }),
    ).toBeInTheDocument();
  });

  it("loads the Agent workspace only when Agent mode is chosen", async () => {
    render(createElement(Workbench));
    await settleLazyModules();
    expect(evaluated.has("AgentWorkbenchBody")).toBe(false);

    fireEvent.click(
      within(screen.getByRole("radiogroup", { name: "Window mode" })).getByRole(
        "radio",
        { name: "Agent" },
      ),
    );
    await settleLazyModules();

    expect(evaluated.has("AgentWorkbenchBody")).toBe(true);
    expect(
      await screen.findByRole("complementary", { name: "Projects and chats" }),
    ).toBeInTheDocument();
  });

  it("loads the Diff canvas on first open and keeps its read-only line comments for the owning Chat", async () => {
    seedSnapshotOwner();
    vi.spyOn(acpDesktop, "getFileSnapshot").mockResolvedValue({
      ...snapshotRef,
      projectionRevision: "1" as never,
      oldText: "old\n",
      newText: "new\n",
      oldBytes: 4,
      newBytes: 4,
    });
    useWorkbenchStore.setState({
      flushActiveEditor: vi.fn(async () => undefined),
      revealFile: vi.fn(async () => undefined),
      tabs: [
        {
          id: snapshotRef.chatId,
          kind: "chat",
          title: snapshotPage().title,
          position: 0,
          pinned: true,
          dirty: false,
        },
      ],
      activeTabId: snapshotRef.chatId,
      chatOwnershipEpoch: 1,
      chatUiById: {
        [snapshotRef.chatId]: {
          composerText: "",
          attachedContext: [],
          sending: false,
          error: null,
          errorKind: null,
          requestGeneration: 0,
        },
      },
    });
    await openAgentArtifact({
      owner: snapshotRef,
      target: "diff",
      resource: snapshotResource,
      diffRef: snapshotRef,
    });
    pinAgentChatOwner(snapshotRef.chatId);
    const diffTab = useWorkbenchStore
      .getState()
      .tabs.find((tab) => tab.kind === "diff")!;
    registerWorkbenchSurfaces();
    expect(evaluated.has("DiffSurface")).toBe(false);

    const readOnly = render(
      createElement(
        "div",
        null,
        renderFileOrDiffCanvas(diffTab, { readOnly: true }),
      ),
    );
    await settleLazyModules();
    expect(evaluated.has("DiffSurface")).toBe(true);
    expect(
      await readOnly.findByRole("table", { name: "Unified file diff" }),
    ).toBeVisible();
    expect(
      readOnly.getByRole("button", { name: "Comment on old line 1" }),
    ).toBeInTheDocument();
    readOnly.unmount();

    const editable = render(
      createElement("div", null, renderFileOrDiffCanvas(diffTab)),
    );
    await settleLazyModules();
    expect(
      await editable.findByRole("table", { name: "Unified file diff" }),
    ).toBeVisible();
    expect(
      editable.queryAllByRole("button", { name: /Comment on/ }),
    ).toHaveLength(0);
  });

  it("loads the diagram renderer only for a diagram preview", async () => {
    const markdown = render(
      createElement(FileMarkdownPreview, {
        source: "# Notes\n\nPlain text.\n",
      }),
    );
    await settleLazyModules();
    expect(
      markdown.getByRole("heading", { name: "Notes" }),
    ).toBeInTheDocument();
    expect(evaluated.has("MermaidFigure")).toBe(false);
    markdown.unmount();

    render(
      createElement(FileMarkdownPreview, {
        source: "flowchart LR\n  Start --> Finish\n",
        asDiagram: true,
      }),
    );
    await settleLazyModules();
    expect(evaluated.has("MermaidFigure")).toBe(true);
    const diagram = screen.getByRole("article", { name: "Diagram preview" });
    expect(await within(diagram).findByText("Start")).toBeInTheDocument();
    expect(within(diagram).getByText("Finish")).toBeInTheDocument();
  });
  it("loads the terminal only when opened and scopes it to the pinned Agent chat", async () => {
    native.tauri = true;
    const chat = (id: string, position: number) => ({
      id,
      kind: "chat" as const,
      title: id,
      position,
      pinned: false,
      dirty: false,
    });
    useWorkbenchStore.setState({
      phase: "ready",
      project: { id: "project-a", name: "A", roots: [] },
      nativeTransitionEpoch: 7,
      tabs: [chat("chat:first", 0), chat("chat:pinned", 1)],
      activeTabId: null,
    });
    pinAgentChatOwner("chat:pinned");
    render(createElement(Workbench));
    await settleLazyModules();
    expect(evaluated.has("TerminalDockShell")).toBe(false);
    expect(evaluated.has("TerminalDock")).toBe(false);

    fireEvent.click(
      await screen.findByRole("button", { name: "Toggle terminal" }),
    );
    await settleLazyModules();

    expect(
      await screen.findByText("Shell f99cd56a-32d9-4600-a1f6-000000000001"),
    ).toBeInTheDocument();
    expect(evaluated.has("TerminalDockShell")).toBe(true);
    expect(evaluated.has("TerminalDock")).toBe(true);
    expect(native.terminalScopes).toEqual([
      { projectId: "project-a", projectGeneration: "7", chatId: "chat:pinned" },
    ]);
  });
});

import { mockIPC } from "@tauri-apps/api/mocks";
import type { InvokeArgs } from "@tauri-apps/api/core";
import { afterEach, describe, expect, it } from "vitest";

import { createBrowserWorkbenchPorts } from "./browserWorkbenchPorts";
import {
  getWorkbenchPorts as getDesktopWorkbenchPorts,
  resetWorkbenchPortsForTests as resetDesktopWorkbenchPortsForTests,
} from "./workbenchRuntime.desktop";
import {
  getWorkbenchPorts,
  resetWorkbenchPortsForTests,
} from "./workbenchRuntime";
import type { WorkbenchPorts } from "./workbenchPorts";
import {
  selectAgentSession,
  selectEditorDocument,
  selectOpenProject,
  selectRepositoryStatus,
  selectActiveChatId,
} from "../app/workbenchSelectors";
import type { WorkbenchState } from "../app/store";
import type { ResourceRef } from "../domain/workbench";
import { resetBrowserDesktopForTests } from "../services/browserDesktop";

const resource: ResourceRef = { rootId: "root-a", relativePath: "src/a.ts" };

// A stand-in for the desktop process at the Tauri IPC boundary: each command
// answers from what it was sent, so an adapter that drops or rewrites its
// arguments, or never reaches the backend, yields a different value.
function installDesktopBackend() {
  mockIPC((command: string, payload?: InvokeArgs) => {
    const args = (payload ?? {}) as Record<string, unknown>;
    switch (command) {
      case "get_bootstrap_snapshot":
        return { project: { id: "desktop-project" }, transitionEpoch: 9 };
      case "open_editor_document": {
        const opened = args.resource as ResourceRef;
        return {
          resource: opened,
          buffer: { content: `desktop bytes of ${opened.relativePath}` },
        };
      }
      case "create_chat":
        return {
          committed: true,
          transitionEpoch: (args.expectedTransitionEpoch as number) + 1,
          projectId: args.projectId,
          chatId: args.chatId,
        };
      case "get_editor_settings":
        return { "editor.fontSize": 21 };
      default:
        throw new Error(`unexpected desktop command ${command}`);
    }
  });
}

afterEach(() => {
  delete (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
  resetWorkbenchPortsForTests();
  resetDesktopWorkbenchPortsForTests();
  resetBrowserDesktopForTests();
});

async function chatLayout(ports: WorkbenchPorts) {
  const snapshot = await ports.project.getBootstrapSnapshot();
  return {
    snapshot,
    layout: {
      projectId: snapshot.project!.id,
      tabs: snapshot.tabs,
      activeTabId: snapshot.activeTabId,
      selectedRootId: snapshot.selectedRootId,
      selectedResource: snapshot.selectedResource,
    },
  };
}

describe("S0.3 workbench port seam", () => {
  it("serves a plain browser page from the in-page workbench fixture", async () => {
    const ports = getWorkbenchPorts();

    const { snapshot, layout } = await chatLayout(ports);
    expect(snapshot.project?.id).toBe("browser-project");

    const document = await ports.editor.openEditorDocument(resource);
    expect(document.buffer.content).toBe(
      "// Browser projection for src/a.ts\n",
    );

    const created = await ports.chat.createChat(
      layout,
      snapshot.transitionEpoch,
      undefined,
      "33333333-3333-4333-8333-333333333333",
    );
    expect(created.committed).toBe(true);
    expect(created.transitionEpoch).toBe(snapshot.transitionEpoch + 1);
    const newChatId = created.snapshot!.activeTabId;
    expect(newChatId).toBe("33333333-3333-4333-8333-333333333333");
    expect(
      (await ports.project.getBootstrapSnapshot()).tabs.map((tab) => tab.id),
    ).toContain(newChatId);

    const before = await ports.settings.getEditorSettings();
    await ports.settings.setEditorSettings({ "editor.fontSize": 16 });
    expect(
      (await getWorkbenchPorts().settings.getEditorSettings())[
        "editor.fontSize"
      ],
    ).toBe(16);
    await ports.settings.setEditorSettings(before);
  });

  it("routes the same operations to the desktop backend inside the desktop app", async () => {
    installDesktopBackend();
    const ports = getWorkbenchPorts();

    const snapshot = await ports.project.getBootstrapSnapshot();
    expect(snapshot.project?.id).toBe("desktop-project");
    expect(
      (await ports.editor.openEditorDocument(resource)).buffer.content,
    ).toBe("desktop bytes of src/a.ts");
    expect(
      await ports.chat.createChat(
        {
          projectId: "desktop-project",
          tabs: [],
          activeTabId: null,
          selectedRootId: null,
          selectedResource: null,
        },
        9,
        "desktop-project",
        "44444444-4444-4444-8444-444444444444",
      ),
    ).toMatchObject({
      committed: true,
      transitionEpoch: 10,
      projectId: "desktop-project",
      chatId: "44444444-4444-4444-8444-444444444444",
    });
    expect(await ports.settings.getEditorSettings()).toEqual({
      "editor.fontSize": 21,
    });
  });

  it("keeps the desktop-only facade on the desktop backend, never the browser fixture", async () => {
    await expect(
      getDesktopWorkbenchPorts().project.getBootstrapSnapshot(),
    ).rejects.toBeInstanceOf(Error);

    installDesktopBackend();
    resetDesktopWorkbenchPortsForTests();
    const ports = getDesktopWorkbenchPorts();
    expect((await ports.project.getBootstrapSnapshot()).project?.id).toBe(
      "desktop-project",
    );
    expect(
      (await ports.editor.openEditorDocument(resource)).buffer.content,
    ).toBe("desktop bytes of src/a.ts");
  });

  it("browser Editor settings refuse a wrong type like the desktop store", async () => {
    const { settings } = createBrowserWorkbenchPorts();
    const before = await settings.getEditorSettings();

    await expect(
      settings.setEditorSettings({
        "editor.fontSize": "16",
      } as unknown as Parameters<typeof settings.setEditorSettings>[0]),
    ).rejects.toThrow("editor.fontSize");
    expect(await settings.getEditorSettings()).toEqual(before);
    await expect(
      settings.setEditorSettings({
        "editor.fontSize": 15,
      } as unknown as Parameters<typeof settings.setEditorSettings>[0]),
    ).resolves.toMatchObject({ "editor.fontSize": 14 });
  });

  it("exposes named store selectors so features do not expand aggregate store", () => {
    const state = {
      project: { id: "p", name: "P", roots: [] },
      sessionId: "session",
      selectedModelId: "sim-fast",
      repositories: { roots: [], truncated: false, elapsedMs: 0 },
      activeTabId: "chat:a",
      tabs: [
        {
          id: "chat:a",
          kind: "chat",
          title: "Chat",
          position: 0,
          pinned: false,
          dirty: false,
        },
      ],
      activeEditor: {
        phase: "ready",
        resource: { rootId: "r", relativePath: "a.ts" },
        document: {
          resource: { rootId: "r", relativePath: "a.ts" },
          base: { hash: "h", byteLength: 0, lineEnding: "lf" },
          lineEnding: "lf",
          content: "",
          recovered: false,
        },
        content: "",
        revision: 1,
        persistedRevision: 1,
        persisting: false,
        saving: false,
        message: null,
      },
    } as unknown as WorkbenchState;
    expect(selectOpenProject(state)?.id).toBe("p");
    expect(selectAgentSession(state)).toEqual({
      sessionId: "session",
      modelId: "sim-fast",
    });
    expect(selectRepositoryStatus(state).elapsedMs).toBe(0);
    expect(selectActiveChatId(state)).toBe("chat:a");
    expect(selectEditorDocument(state)?.resource.relativePath).toBe("a.ts");
  });
});

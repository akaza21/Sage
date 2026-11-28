import { mockIPC } from "@tauri-apps/api/mocks";
import type { InvokeArgs } from "@tauri-apps/api/core";
import { afterEach, describe, expect, it } from "vitest";

import type { ResourceRef } from "../domain/workbench";
// Vite's simulator resolver leaves this file on the production module.
import {
  getWorkbenchPorts,
  hasWorkbenchBackend,
  resetWorkbenchPortsForTests,
} from "./workbenchRuntime";

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
});

describe("production workbench runtime", () => {
  it("has no backend in a plain browser page and never falls back to fake data", async () => {
    expect(hasWorkbenchBackend()).toBe(false);
    await expect(
      getWorkbenchPorts().project.getBootstrapSnapshot(),
    ).rejects.toBeInstanceOf(Error);
  });

  it("routes the same operations to the desktop backend inside the desktop app", async () => {
    installDesktopBackend();
    expect(hasWorkbenchBackend()).toBe(true);
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
});

import { describe, expect, it } from "vitest";

import { FROZEN_WORKBENCH_PORT_METHODS } from "./workbenchPorts";
import { createBrowserWorkbenchPorts } from "./browserWorkbenchPorts";
import { createNativeWorkbenchPorts } from "./nativeWorkbenchPorts";
import {
  getWorkbenchPorts as getDesktopWorkbenchPorts,
  resetWorkbenchPortsForTests as resetDesktopWorkbenchPortsForTests,
} from "./workbenchRuntime.desktop";
import { getWorkbenchPorts } from "./workbenchRuntime";
import {
  selectAgentSession,
  selectEditorDocument,
  selectOpenProject,
  selectRepositoryStatus,
  selectActiveChatId,
} from "../app/workbenchSelectors";
import type { WorkbenchState } from "../app/store";

describe("S0.3 workbench port seam", () => {
  it("native and browser adapters implement every frozen port method", () => {
    const native = createNativeWorkbenchPorts();
    const browser = createBrowserWorkbenchPorts();
    for (const portName of Object.keys(FROZEN_WORKBENCH_PORT_METHODS) as Array<
      keyof typeof FROZEN_WORKBENCH_PORT_METHODS
    >) {
      const methods = FROZEN_WORKBENCH_PORT_METHODS[portName];
      const nativePort = native[portName] as unknown as Record<string, unknown>;
      const browserPort = browser[portName] as unknown as Record<
        string,
        unknown
      >;
      for (const method of methods) {
        expect(typeof nativePort[method], `native.${portName}.${method}`).toBe(
          "function",
        );
        expect(
          typeof browserPort[method],
          `browser.${portName}.${method}`,
        ).toBe("function");
      }
    }
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

  it("exposes getWorkbenchPorts as the runtime acquisition seam", () => {
    const ports = getWorkbenchPorts();
    expect(typeof ports.project.getBootstrapSnapshot).toBe("function");
    expect(typeof ports.editor.openEditorDocument).toBe("function");
    expect(typeof ports.chat.createChat).toBe("function");
    expect(typeof ports.settings.getEditorSettings).toBe("function");
  });

  it("keeps the desktop-only facade synchronous and native-only", () => {
    resetDesktopWorkbenchPortsForTests();
    const first = getDesktopWorkbenchPorts();
    expect(first).toBe(getDesktopWorkbenchPorts());
    expect(typeof first.project.getBootstrapSnapshot).toBe("function");
    expect(typeof first.editor.openEditorDocument).toBe("function");
    expect(typeof first.chat.createChat).toBe("function");
    resetDesktopWorkbenchPortsForTests();
    expect(getDesktopWorkbenchPorts()).not.toBe(first);
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

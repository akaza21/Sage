import { readFile, readdir } from "node:fs/promises";
import path from "node:path";
import { describe, expect, it } from "vitest";

import { FROZEN_WORKBENCH_PORT_METHODS } from "./workbenchPorts";
import { createBrowserWorkbenchPorts } from "./browserWorkbenchPorts";
import { createNativeWorkbenchPorts } from "./nativeWorkbenchPorts";
import { getWorkbenchPorts } from "./workbenchRuntime";
import {
  selectAgentSession,
  selectEditorDocument,
  selectOpenProject,
  selectRepositoryStatus,
  selectActiveChatId,
} from "../app/workbenchSelectors";
import type { WorkbenchState } from "../app/store";

const srcRoot = path.resolve(import.meta.dirname, "..");

async function featureProductionFiles(): Promise<string[]> {
  const files: string[] = [];
  async function walk(relative: string) {
    const entries = await readdir(path.join(srcRoot, relative), {
      withFileTypes: true,
    });
    for (const entry of entries) {
      const next = `${relative}/${entry.name}`;
      if (entry.isDirectory()) {
        await walk(next);
        continue;
      }
      if (
        /\.(?:ts|tsx)$/.test(entry.name) &&
        !/\.test\.tsx?$/.test(entry.name)
      ) {
        files.push(next);
      }
    }
  }
  await walk("features");
  return files;
}

describe("S0.3 workbench port seam", () => {
  it("freezes the six port method names", () => {
    expect(Object.keys(FROZEN_WORKBENCH_PORT_METHODS).sort()).toEqual([
      "agent",
      "chat",
      "editor",
      "project",
      "repository",
      "settings",
    ]);
    expect(FROZEN_WORKBENCH_PORT_METHODS.project).toEqual([
      "addProjectRootFromPicker",
      "createProjectFile",
      "createProjectFolder",
      "createProjectFromPicker",
      "ensureLocalProject",
      "getBootstrapSnapshot",
      "listDirectory",
      "persistProjectLayout",
      "persistProjectLayoutAndClose",
      "renameProjectFile",
      "renameProjectFolder",
      "switchProject",
      "trashProjectResource",
      "unwatchWorkspaceNotify",
      "watchWorkspaceNotify",
    ]);
    expect(FROZEN_WORKBENCH_PORT_METHODS.editor).toEqual([
      "discardEditorBuffer",
      "openEditorDocument",
      "readTextFile",
      "saveEditorDocument",
      "snapshotEditorBuffer",
      "watchEditorDocument",
    ]);
    expect(FROZEN_WORKBENCH_PORT_METHODS.repository).toEqual([
      "abortGitInProgress",
      "checkoutGitLocalBranch",
      "commitGitRepository",
      "continueGitCherryPick",
      "continueGitRebase",
      "continueGitRevert",
      "createGitLocalBranch",
      "deleteGitLocalBranch",
      "discoverProjectRepositories",
      "fetchGitRemote",
      "getFileDiff",
      "getNestedRepositoryFileDiff",
      "getNestedRepositoryStatus",
      "getRepositoryStatus",
      "mutateGitFile",
      "pushGitRemote",
      "renameGitLocalBranch",
    ]);
    expect(FROZEN_WORKBENCH_PORT_METHODS.chat).toEqual([
      "activateChat",
      "archiveChat",
      "createChat",
      "moveChat",
      "pinChat",
      "renameChat",
      "restoreChat",
    ]);
    expect(FROZEN_WORKBENCH_PORT_METHODS.agent).toEqual([
      "listHarnessModels",
      "sendPrompt",
    ]);
    expect(FROZEN_WORKBENCH_PORT_METHODS.settings).toEqual([
      "getEditorAppearance",
      "setEditorAppearance",
    ]);
  });

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

  it("exposes getWorkbenchPorts as the runtime acquisition seam", () => {
    const ports = getWorkbenchPorts();
    expect(typeof ports.project.getBootstrapSnapshot).toBe("function");
    expect(typeof ports.editor.openEditorDocument).toBe("function");
    expect(typeof ports.chat.createChat).toBe("function");
    expect(typeof ports.settings.getEditorAppearance).toBe("function");
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

  it("keeps feature production files off the aggregate desktop object", async () => {
    const files = await featureProductionFiles();
    expect(files.length).toBeGreaterThan(0);
    const offenders: string[] = [];
    for (const file of files) {
      const source = await readFile(path.join(srcRoot, file), "utf8");
      if (
        /from ["'](?:\.\.\/)+services\/desktop["']/.test(source) ||
        /from ["']@?\/?src\/services\/desktop["']/.test(source)
      ) {
        offenders.push(file);
      }
    }
    expect(offenders).toEqual([]);
  });
});

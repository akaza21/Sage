// @vitest-environment node

import path from "node:path";
import { build, resolveConfig, type InlineConfig } from "vite";
import { describe, expect, it } from "vitest";

import {
  DESKTOP_NATIVE_BUILD_MODE,
  desktopNativeWorkbenchRuntimePlugin,
  resolveDesktopNativeWorkbenchRuntime,
} from "./vite.config";
import viteConfig from "./vite.config";

const root = path.resolve(import.meta.dirname);
const importer = path.join(root, "src/app/store.ts");
const runtime = path.join(root, "src/ports/workbenchRuntime.desktop.ts");

const INITIAL_GROUPS = [{ name: "initial-deps", tags: ["$initial"] }];
const EXPECTED_DYNAMIC_FACADES = [
  "node_modules/@codemirror/lang-html/dist/index.js",
  "node_modules/@codemirror/lang-json/dist/index.js",
  "node_modules/@codemirror/lang-markdown/dist/index.js",
  "node_modules/@codemirror/lang-python/dist/index.js",
  "node_modules/@codemirror/lang-rust/dist/index.js",
  "node_modules/@tauri-apps/api/core.js",
  "node_modules/@tauri-apps/api/event.js",
  "node_modules/@tauri-apps/api/window.js",
  "src/app/agentDiffAdmission.ts",
  "src/app/agentDiffLoad.ts",
  "src/features/chat/acp/AcpChatPresentation.tsx",
  "src/features/chat/acp/MermaidFigure.tsx",
  "src/features/diff/DiffSurface.tsx",
  "src/features/files/CodeMirrorEditorRuntime.ts",
  "src/features/files/FileSurface.tsx",
  "src/features/preview/AgentPreviewPresentation.tsx",
  "src/features/preview/PreviewSurface.tsx",
  "src/features/search/SearchSurface.tsx",
  "src/services/acpFileSnapshot.ts",
  "src/workbench/AgentWorkbenchBody.tsx",
  "src/workbench/SettingsSurface.tsx",
  "src/workbench/SourceControlView.tsx",
];

type DesktopRolldownOutput = NonNullable<
  NonNullable<NonNullable<InlineConfig["build"]>["rolldownOptions"]>["output"]
>;

function codeSplittingGroups(output: DesktopRolldownOutput | undefined) {
  if (!output || Array.isArray(output)) return undefined;
  const codeSplitting = output.codeSplitting;
  if (!codeSplitting || typeof codeSplitting !== "object") return undefined;
  return codeSplitting.groups;
}

function initialGroupConfig(command: "build" | "serve", mode: string) {
  return viteConfig({ command, mode });
}

function desktopGraphConfig(withInitialGroup: boolean): InlineConfig {
  const configured = initialGroupConfig("build", DESKTOP_NATIVE_BUILD_MODE);
  const buildConfig = { ...configured.build };
  if (!withInitialGroup) delete buildConfig.rolldownOptions;
  return {
    ...configured,
    configFile: false,
    mode: DESKTOP_NATIVE_BUILD_MODE,
    root,
    build: {
      ...buildConfig,
      emptyOutDir: false,
      write: false,
    },
  };
}

function graphInventory(result: Awaited<ReturnType<typeof build>>) {
  const outputs = Array.isArray(result) ? result : [result];
  const chunks = outputs
    .flatMap((output) => ("output" in output ? output.output : []))
    .filter((asset) => asset.type === "chunk");
  const byFileName = new Map(chunks.map((chunk) => [chunk.fileName, chunk]));
  const initialFiles = new Set(
    chunks.filter((chunk) => chunk.isEntry).map((chunk) => chunk.fileName),
  );
  const pending = [...initialFiles];
  while (pending.length > 0) {
    const fileName = pending.pop();
    if (!fileName) continue;
    const chunk = byFileName.get(fileName);
    for (const imported of chunk?.imports ?? []) {
      if (!initialFiles.has(imported)) {
        initialFiles.add(imported);
        pending.push(imported);
      }
    }
  }
  const modules = (entries: readonly (typeof chunks)[number][]) =>
    entries.flatMap((chunk) => Object.keys(chunk.modules)).sort();
  return {
    allModules: modules(chunks),
    dynamicFacades: chunks
      .filter((chunk) => chunk.isDynamicEntry && chunk.facadeModuleId)
      .map((chunk) =>
        path.relative(root, chunk.facadeModuleId!).split(path.sep).join("/"),
      )
      .sort(),
    entryCount: chunks.filter((chunk) => chunk.isEntry).length,
    initialModules: modules(
      [...initialFiles]
        .map((fileName) => byFileName.get(fileName))
        .filter((chunk): chunk is (typeof chunks)[number] => Boolean(chunk)),
    ),
  };
}

describe("desktop-native runtime resolver", () => {
  it("redirects only the canonical runtime import during the desktop build", () => {
    expect(
      resolveDesktopNativeWorkbenchRuntime(
        "build",
        DESKTOP_NATIVE_BUILD_MODE,
        "../ports/workbenchRuntime",
        importer,
      ),
    ).toBe(runtime);
    expect(
      resolveDesktopNativeWorkbenchRuntime(
        "build",
        DESKTOP_NATIVE_BUILD_MODE,
        "../ports/nativeWorkbenchPorts",
        importer,
      ),
    ).toBeUndefined();
  });

  it("leaves development and non-desktop builds on the dual runtime facade", () => {
    expect(
      resolveDesktopNativeWorkbenchRuntime(
        "serve",
        DESKTOP_NATIVE_BUILD_MODE,
        "../ports/workbenchRuntime",
        importer,
      ),
    ).toBeUndefined();
    expect(
      resolveDesktopNativeWorkbenchRuntime(
        "build",
        "development",
        "../ports/workbenchRuntime",
        importer,
      ),
    ).toBeUndefined();
    expect(
      desktopNativeWorkbenchRuntimePlugin("serve", DESKTOP_NATIVE_BUILD_MODE),
    ).toBeUndefined();
  });
});

describe("desktop-native initial dependency grouping", () => {
  it("enables exactly one initial-only group only for the desktop-native build", async () => {
    expect(typeof Range).toBe("undefined");
    const desktopBuild = await resolveConfig(
      desktopGraphConfig(true),
      "build",
      DESKTOP_NATIVE_BUILD_MODE,
    );
    const desktopServe = initialGroupConfig("serve", DESKTOP_NATIVE_BUILD_MODE);
    const browserBuild = initialGroupConfig("build", "development");

    expect(
      codeSplittingGroups(desktopBuild.build.rolldownOptions?.output),
    ).toEqual(INITIAL_GROUPS);
    expect(
      codeSplittingGroups(desktopServe.build?.rolldownOptions?.output),
    ).toBeUndefined();
    expect(
      codeSplittingGroups(browserBuild.build?.rolldownOptions?.output),
    ).toBeUndefined();
  });

  it("preserves the complete desktop-native module graph and lazy facade oracle in memory", async () => {
    const ordinaryConfig = await resolveConfig(
      desktopGraphConfig(false),
      "build",
      DESKTOP_NATIVE_BUILD_MODE,
    );
    expect(
      codeSplittingGroups(ordinaryConfig.build.rolldownOptions?.output),
    ).toBeUndefined();

    const [ordinary, grouped] = await Promise.all([
      build(desktopGraphConfig(false)),
      build(desktopGraphConfig(true)),
    ]);
    const ordinaryGraph = graphInventory(ordinary);
    const groupedGraph = graphInventory(grouped);

    expect(ordinaryGraph.entryCount).toBe(1);
    expect(groupedGraph.entryCount).toBe(1);
    expect(ordinaryGraph.allModules).toHaveLength(414);
    expect(ordinaryGraph.initialModules).toHaveLength(263);
    for (const snapshotModule of [
      "/src/domain/agentDiff.ts",
      "/src/services/acpFileSnapshot.ts",
      "/src/app/agentDiffAdmission.ts",
      "/src/app/agentDiffLoad.ts",
      "/src/features/diff/snapshotDiff.ts",
    ]) {
      expect(
        ordinaryGraph.allModules.some((id) => id.endsWith(snapshotModule)),
      ).toBe(true);
      expect(
        ordinaryGraph.initialModules.some((id) => id.endsWith(snapshotModule)),
      ).toBe(false);
    }
    expect(groupedGraph.allModules).toEqual(ordinaryGraph.allModules);
    expect(groupedGraph.initialModules).toEqual(ordinaryGraph.initialModules);
    expect(groupedGraph.dynamicFacades).toEqual(EXPECTED_DYNAMIC_FACADES);
    expect(ordinaryGraph.dynamicFacades).toEqual(EXPECTED_DYNAMIC_FACADES);
  }, 90_000);
});

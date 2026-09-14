// @vitest-environment node

import path from "node:path";
import { build, resolveConfig, type InlineConfig } from "vite";
import { describe, expect, it } from "vitest";

import {
  DESKTOP_NATIVE_BUILD_MODE,
  DESKTOP_NATIVE_CODE_SPLITTING_GROUPS,
  desktopNativeWorkbenchRuntimePlugin,
  resolveDesktopNativeWorkbenchRuntime,
} from "./vite.config";
import viteConfig from "./vite.config";

const root = path.resolve(import.meta.dirname);
const importer = path.join(root, "src/app/store.ts");
const runtime = path.join(root, "src/ports/workbenchRuntime.desktop.ts");

const DESKTOP_GROUPS = DESKTOP_NATIVE_CODE_SPLITTING_GROUPS;
const BROWSER_FIXTURE_MODULE = "/src/services/browserWorkspaceFixture.ts";
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

const CODEMIRROR_LANGUAGE_PARSER_ALLOWLIST = {
  css: ["@lezer/css"],
  html: ["@lezer/css", "@lezer/html", "@lezer/javascript"],
  json: ["@lezer/json"],
  markdown: [
    "@lezer/css",
    "@lezer/html",
    "@lezer/javascript",
    "@lezer/markdown",
  ],
  python: ["@lezer/python"],
  rust: ["@lezer/rust"],
} as const;

type EditorLanguageWithParser =
  keyof typeof CODEMIRROR_LANGUAGE_PARSER_ALLOWLIST;

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
  const modules = (entries: readonly (typeof chunks)[number][]) =>
    entries.flatMap((chunk) => Object.keys(chunk.modules)).sort();
  const staticChunkClosure = (entry: (typeof chunks)[number]) => {
    const files = new Set([entry.fileName]);
    const pending = [entry.fileName];
    while (pending.length > 0) {
      const fileName = pending.pop();
      if (!fileName) continue;
      const chunk = byFileName.get(fileName);
      for (const imported of chunk?.imports ?? []) {
        if (!files.has(imported)) {
          files.add(imported);
          pending.push(imported);
        }
      }
    }
    return modules(
      [...files]
        .map((fileName) => byFileName.get(fileName))
        .filter((chunk): chunk is (typeof chunks)[number] => Boolean(chunk)),
    );
  };
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
  const dynamicFiles = new Set(chunks.flatMap((chunk) => chunk.dynamicImports));
  const dynamicStaticClosures = chunks
    .filter((chunk) => chunk.isDynamicEntry || dynamicFiles.has(chunk.fileName))
    .map((chunk) => ({
      facadeModuleId: chunk.facadeModuleId
        ? path.relative(root, chunk.facadeModuleId).split(path.sep).join("/")
        : undefined,
      modules: Object.keys(chunk.modules).sort(),
      staticClosure: staticChunkClosure(chunk),
    }));
  return {
    allModules: modules(chunks),
    dynamicFacades: dynamicStaticClosures
      .flatMap(({ facadeModuleId }) => (facadeModuleId ? [facadeModuleId] : []))
      .sort(),
    dynamicStaticClosures,
    entryCount: chunks.filter((chunk) => chunk.isEntry).length,
    initialModules: modules(
      [...initialFiles]
        .map((fileName) => byFileName.get(fileName))
        .filter((chunk): chunk is (typeof chunks)[number] => Boolean(chunk)),
    ),
  };
}

function parserPackages(closure: readonly string[]): string[] {
  return closure
    .map((id) => id.replaceAll("\\", "/"))
    .flatMap((id) => {
      const match = id.match(
        /node_modules\/@lezer\/(css|html|javascript|json|markdown|python|rust)\//,
      );
      return match ? [`@lezer/${match[1]}`] : [];
    })
    .filter((value, index, values) => values.indexOf(value) === index)
    .sort();
}

function languageStaticClosure(
  graph: ReturnType<typeof graphInventory>,
  language: EditorLanguageWithParser,
): readonly string[] {
  const languageModule = `node_modules/@codemirror/lang-${language}/dist/index.js`;
  const matches = graph.dynamicStaticClosures.filter(({ modules }) =>
    modules.some((id) => id.endsWith(languageModule)),
  );
  expect(matches).toHaveLength(1);
  return matches[0]!.staticClosure;
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

describe("desktop-native chunk topology", () => {
  it("enables bounded desktop groups only for the desktop-native build", async () => {
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
    ).toEqual(DESKTOP_GROUPS);
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
    // P-02's typed no-content acknowledgement bridge is production-reachable.
    // AGENT-RIGHT-PANE adds one bounded lifecycle presentation component;
    // its CSS is chained through an existing initial stylesheet module.
    // AP-U2 adds two bounded center-chat modules: the tab strip and task header.
    expect(ordinaryGraph.allModules).toHaveLength(418);
    expect(ordinaryGraph.initialModules).toHaveLength(264);
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
    expect(
      groupedGraph.allModules.some((id) => id.endsWith(BROWSER_FIXTURE_MODULE)),
    ).toBe(false);
    expect(groupedGraph.dynamicFacades).toEqual(EXPECTED_DYNAMIC_FACADES);
    expect(ordinaryGraph.dynamicFacades).toEqual(EXPECTED_DYNAMIC_FACADES);

    const editorRuntimeClosure = groupedGraph.dynamicStaticClosures.find(
      ({ facadeModuleId }) =>
        facadeModuleId === "src/features/files/CodeMirrorEditorRuntime.ts",
    )?.staticClosure;
    expect(editorRuntimeClosure).toBeDefined();
    expect(
      editorRuntimeClosure?.some((id) =>
        id.includes("node_modules/@codemirror/state/"),
      ),
    ).toBe(true);
    expect(
      editorRuntimeClosure?.some((id) =>
        id.includes("node_modules/@codemirror/lang-"),
      ),
    ).toBe(false);
    expect(parserPackages(editorRuntimeClosure ?? [])).toEqual([]);

    for (const [language, allowlist] of Object.entries(
      CODEMIRROR_LANGUAGE_PARSER_ALLOWLIST,
    ) as [EditorLanguageWithParser, readonly string[]][]) {
      expect(
        parserPackages(languageStaticClosure(groupedGraph, language)),
      ).toEqual(allowlist);
    }
  }, 90_000);
});

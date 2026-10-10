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
const BASELINE_DESKTOP_GROUPS: typeof DESKTOP_GROUPS = [
  { name: "initial-deps", tags: ["$initial"], priority: 2 },
  {
    name: "workbench-deps",
    priority: 1,
    test: /src[\\/]workbench[\\/]Workbench\.tsx$/,
  },
  {
    name: "codemirror-deps",
    priority: 1,
    test: /node_modules[\\/]@codemirror[\\/](?:state|view|language|commands|autocomplete)[\\/]/,
  },
];
const BROWSER_FIXTURE_MODULE = "/src/services/browserWorkspaceFixture.ts";
const CODEMIRROR_LANGUAGE_DYNAMIC_FACADES = {
  css: "src/features/files/editorLanguageCss.ts",
  html: "src/features/files/editorLanguageHtml.ts",
  javascript: "src/features/files/editorLanguageJavaScript.ts",
  json: "src/features/files/editorLanguageJson.ts",
  markdown: "src/features/files/editorLanguageMarkdown.ts",
  python: "src/features/files/editorLanguagePython.ts",
  rust: "src/features/files/editorLanguageRust.ts",
} as const;

const REQUIRED_DYNAMIC_FACADES = [
  ...Object.values(CODEMIRROR_LANGUAGE_DYNAMIC_FACADES),
  "node_modules/@tauri-apps/api/core.js",
  "node_modules/@tauri-apps/api/event.js",
  "src/app/agentDiffAdmission.ts",
  "src/app/agentDiffLoad.ts",
  "src/features/browser/BrowserSurface.tsx",
  "src/features/chat/acp/AcpChatPresentation.tsx",
  "src/features/chat/acp/MermaidFigure.tsx",
  "src/features/chat/chatImageTransfer.ts",
  "src/features/diff/DiffSurface.tsx",
  "src/features/files/CodeMirrorEditorRuntime.ts",
  "src/features/files/FileSurface.tsx",
  "src/features/preview/AgentPreviewPresentation.tsx",
  "src/features/preview/PreviewSurface.tsx",
  "src/features/search/SearchSurface.tsx",
  "src/services/acpFileSnapshot.ts",
  "src/workbench/SettingsSurface.tsx",
  "src/workbench/SourceControlView.tsx",
  "src/workbench/SubagentSurface.tsx",
  "src/workbench/TerminalDock.tsx",
  "src/workbench/TerminalDockShell.tsx",
].sort();

const ORDINARY_DYNAMIC_FACADES = [
  ...REQUIRED_DYNAMIC_FACADES,
  "src/workbench/AgentWorkbenchBody.tsx",
  "src/workbench/Workbench.tsx",
].sort();

const CONSOLIDATED_AGENT_MODULES = [
  "src/app/subagentPaneStore.ts",
  "src/features/chat/acp/chatLiveSnapshot.ts",
  "src/features/chat/acp/useAcpChatLiveSnapshot.ts",
  "src/features/chatSearch/ChatSearchControl.tsx",
  "src/features/chatSearch/chatSearchDesktop.ts",
  "src/features/preview/AgentPreviewSurface.tsx",
  "src/features/preview/previewDesktop.ts",
  "src/features/preview/previewPresentation.ts",
  "src/features/preview/previewProjectionStore.ts",
  "src/features/preview/useHarnessBrowseSession.ts",
  "src/workbench/AgentArtifactCollapsedPane.tsx",
  "src/workbench/AgentArtifactHeader.tsx",
  "src/workbench/AgentArtifactPane.tsx",
  "src/workbench/agentArtifactTabPresentation.ts",
  "src/workbench/AgentArtifactTabStrip.tsx",
  "src/workbench/AgentChatPane.tsx",
  "src/workbench/AgentChatPanes.tsx",
  "src/workbench/agentPreviewControllerOwner.ts",
  "src/workbench/AgentRailFilterMenu.tsx",
  "src/workbench/AgentRailSections.tsx",
  "src/workbench/agentRailViewPrefs.ts",
  "src/workbench/AgentTaskHeader.tsx",
  "src/workbench/AgentWorkbenchBody.tsx",
  "src/workbench/artifactChipStats.ts",
  "src/workbench/LeftSidebar.tsx",
  "src/workbench/ProjectChatActionMenu.tsx",
  "src/workbench/ProjectChatNavigation.tsx",
  "src/workbench/ProjectChatRailGlyph.tsx",
  "src/workbench/useAgentArtifactFollowLifecycle.ts",
  "src/workbench/useSubagentPaneExpansion.ts",
].sort();

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

function htmlOnlyGraphConfig(): InlineConfig {
  const configured = desktopGraphConfig(true);
  configured.build!.rolldownOptions = {
    output: {
      codeSplitting: {
        groups: DESKTOP_GROUPS.map((group) =>
          group.name === "workbench-deps" ? BASELINE_DESKTOP_GROUPS[1]! : group,
        ),
      },
    },
  };
  return configured;
}

function graphInventory(result: Awaited<ReturnType<typeof build>>) {
  const outputs = Array.isArray(result) ? result : [result];
  const chunks = outputs
    .flatMap((output) => ("output" in output ? output.output : []))
    .filter((asset) => asset.type === "chunk");
  const byFileName = new Map(chunks.map((chunk) => [chunk.fileName, chunk]));
  const modules = (entries: readonly (typeof chunks)[number][]) =>
    entries.flatMap((chunk) => Object.keys(chunk.modules)).sort();
  const staticChunkFiles = (entry: (typeof chunks)[number]) => {
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
    return [...files]
      .map((fileName) => byFileName.get(fileName))
      .filter((chunk): chunk is (typeof chunks)[number] => Boolean(chunk));
  };
  const staticChunkClosure = (entry: (typeof chunks)[number]) =>
    modules(staticChunkFiles(entry));
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
    staticClosuresByFile: new Map(
      chunks.map((chunk) => [chunk.fileName, staticChunkClosure(chunk)]),
    ),
    moduleFiles: new Map(
      chunks.flatMap((chunk) =>
        Object.keys(chunk.modules).map((id) => [
          path.relative(root, id).split(path.sep).join("/"),
          chunk.fileName,
        ]),
      ),
    ),
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
  const facadeModuleId = CODEMIRROR_LANGUAGE_DYNAMIC_FACADES[language];
  const matches = graph.dynamicStaticClosures.filter(
    (closure) => closure.facadeModuleId === facadeModuleId,
  );
  expect(matches).toHaveLength(1);
  return matches[0]!.staticClosure;
}

function closureForModule(
  graph: ReturnType<typeof graphInventory>,
  id: string,
) {
  const file = graph.moduleFiles.get(id);
  const closure = graph.staticClosuresByFile.get(file ?? "");
  if (!closure) throw new Error(`Missing static closure for ${id}`);
  return closure;
}

function consolidatedClosure(
  baseline: ReturnType<typeof graphInventory>,
  id: string,
) {
  const original = closureForModule(baseline, id);
  const workbenchMembers = [
    "src/workbench/Workbench.tsx",
    ...CONSOLIDATED_AGENT_MODULES,
  ];
  if (
    !original.some((module) =>
      workbenchMembers.some((member) =>
        module.replaceAll("\\", "/").endsWith(`/${member}`),
      ),
    )
  ) {
    return original;
  }
  return [
    ...new Set([
      ...original,
      ...closureForModule(baseline, "src/workbench/AgentWorkbenchBody.tsx"),
    ]),
  ].sort();
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
  it("adds only the Agent-body reserve", () => {
    expect(DESKTOP_GROUPS[0]).toEqual(BASELINE_DESKTOP_GROUPS[0]);
    const workbench = DESKTOP_GROUPS.find(
      (group) => group.name === "workbench-deps",
    );
    expect(workbench?.priority).toBe(1);
    if (!(workbench?.test instanceof RegExp))
      throw new Error("Expected a bounded Workbench filter");
    for (const separator of ["/", "\\"]) {
      const id = (module: string) =>
        `${root}/src/workbench/${module}.tsx`.replaceAll("/", separator);
      expect(workbench.test.test(id("Workbench"))).toBe(true);
      expect(workbench.test.test(id("AgentWorkbenchBody"))).toBe(true);
      for (const module of [
        "SubagentSurface",
        "SettingsSurface",
        "SourceControlView",
        "TerminalDock",
        "TerminalDockShell",
      ]) {
        expect(workbench.test.test(id(module)), module).toBe(false);
      }
    }
  });

  it("preserves chat-helper ownership with the exact authorised Agent closure consolidation", async () => {
    const baselineConfig = desktopGraphConfig(true);
    baselineConfig.build!.rolldownOptions = {
      output: { codeSplitting: { groups: BASELINE_DESKTOP_GROUPS } },
    };
    const [baseline, grouped] = await Promise.all([
      build(baselineConfig),
      build(desktopGraphConfig(true)),
    ]);
    const baselineGraph = graphInventory(baseline);
    const groupedGraph = graphInventory(grouped);
    const files = [
      "src/features/chat/acp/TurnEditSummary.tsx",
      "src/features/chat/acp/TurnActivityItem.tsx",
      "src/features/chat/acp/ToolActivityRollupView.tsx",
      "src/features/chat/acp/ChatCodeBlock.tsx",
    ].map((id) => groupedGraph.moduleFiles.get(id));
    expect(files.every((file) => typeof file === "string")).toBe(true);
    expect(new Set(files).size).toBe(1);
    expect(files[0]).toMatch(/chat-ui-/);
    for (const facade of [
      "src/workbench/AgentWorkbenchBody.tsx",
      "src/features/files/CodeMirrorEditorRuntime.ts",
      "src/features/files/FileSurface.tsx",
      "src/features/diff/DiffSurface.tsx",
    ]) {
      expect(closureForModule(groupedGraph, facade), facade).toEqual(
        consolidatedClosure(baselineGraph, facade),
      );
    }
  }, 90_000);

  it("consolidates exactly the Agent body while preserving all initial and other lazy boundaries", async () => {
    const baselineConfig = htmlOnlyGraphConfig();
    const [baseline, grouped] = await Promise.all([
      build(baselineConfig),
      build(desktopGraphConfig(true)),
    ]);
    const baselineGraph = graphInventory(baseline);
    const groupedGraph = graphInventory(grouped);
    const workbench = "src/workbench/Workbench.tsx";
    const agent = "src/workbench/AgentWorkbenchBody.tsx";
    const sharedFile = groupedGraph.moduleFiles.get(workbench);
    const baselineFile = baselineGraph.moduleFiles.get(workbench);
    expect(sharedFile).toMatch(/workbench-deps-/);
    expect(groupedGraph.moduleFiles.get(agent)).toBe(sharedFile);
    const movedModules = [...groupedGraph.moduleFiles]
      .filter(
        ([id, file]) =>
          id.startsWith("src/") &&
          /\.[cm]?[jt]sx?$/.test(id) &&
          file === sharedFile &&
          baselineGraph.moduleFiles.get(id) !== baselineFile,
      )
      .map(([id]) => id)
      .sort();
    expect(movedModules).toEqual(CONSOLIDATED_AGENT_MODULES);
    expect(groupedGraph.allModules).toEqual(baselineGraph.allModules);
    expect(groupedGraph.initialModules).toEqual(baselineGraph.initialModules);
    expect(new Set(groupedGraph.allModules).size).toBe(
      groupedGraph.allModules.length,
    );
    const subagent = "src/workbench/SubagentSurface.tsx";
    const childFile = groupedGraph.moduleFiles.get(subagent);
    expect(childFile).toMatch(/SubagentSurface-/);
    expect(childFile).not.toBe(sharedFile);
    expect(groupedGraph.dynamicFacades).toEqual(REQUIRED_DYNAMIC_FACADES);
    for (const facade of [workbench, agent, ...REQUIRED_DYNAMIC_FACADES]) {
      expect(closureForModule(groupedGraph, facade), facade).toEqual(
        consolidatedClosure(baselineGraph, facade),
      );
    }
    for (const facade of [
      workbench,
      agent,
      "src/features/chat/acp/AcpChatPresentation.tsx",
    ]) {
      expect(
        closureForModule(groupedGraph, facade).some((id) =>
          id.endsWith(subagent),
        ),
        facade,
      ).toBe(false);
    }
    for (const facade of [workbench, agent, subagent]) {
      expect(
        closureForModule(groupedGraph, facade).some((id) =>
          /(?:@codemirror|@xterm|MermaidFigure\.tsx|CodeMirrorEditorRuntime\.ts|TerminalDock\.tsx)/.test(
            id,
          ),
        ),
        facade,
      ).toBe(false);
    }
    for (const facade of Object.values(CODEMIRROR_LANGUAGE_DYNAMIC_FACADES)) {
      expect(closureForModule(groupedGraph, facade), facade).toEqual(
        closureForModule(baselineGraph, facade),
      );
    }
  }, 90_000);

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
    // Preserve the lazy-boundary shape without making every new module a merge
    // conflict: the initial graph must remain a minority of the full graph.
    // The reference's icon-and-value composer row replaces labels that
    // truncated selected values, requiring one model glyph in the initial graph.
    expect(ordinaryGraph.initialModules.length).toBeLessThanOrEqual(281);
    expect(ordinaryGraph.initialModules.length).toBeLessThanOrEqual(
      Math.ceil(ordinaryGraph.allModules.length * 0.65),
    );
    for (const snapshotModule of [
      "/src/domain/agentDiff.ts",
      "/src/services/acpFileSnapshot.ts",
      "/src/app/agentDiffAdmission.ts",
      "/src/app/agentDiffLoad.ts",
      "/src/features/diff/snapshotDiff.ts",
      "/src/workbench/AgentWorkbenchBody.tsx",
      "/src/workbench/SettingsSurface.tsx",
      "/src/workbench/SourceControlView.tsx",
      "/src/workbench/Workbench.tsx",
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
    expect(
      ordinaryGraph.allModules.some((id) =>
        id.endsWith("node_modules/@tauri-apps/api/window.js"),
      ),
    ).toBe(false);
    expect(groupedGraph.dynamicFacades).toEqual(REQUIRED_DYNAMIC_FACADES);
    expect(ordinaryGraph.dynamicFacades).toEqual(ORDINARY_DYNAMIC_FACADES);

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
    expect(
      editorRuntimeClosure?.some((id) =>
        id.includes("node_modules/@lezer/lr/"),
      ),
    ).toBe(false);

    for (const [language, allowlist] of Object.entries(
      CODEMIRROR_LANGUAGE_PARSER_ALLOWLIST,
    ) as [EditorLanguageWithParser, readonly string[]][]) {
      expect(
        parserPackages(languageStaticClosure(groupedGraph, language)),
      ).toEqual(allowlist);
    }
  }, 90_000);
});

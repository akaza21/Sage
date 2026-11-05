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
const LANE_BASE_DESKTOP_GROUPS: typeof DESKTOP_GROUPS = [
  BASELINE_DESKTOP_GROUPS[0]!,
  {
    ...BASELINE_DESKTOP_GROUPS[1]!,
    test: /src[\\/]workbench[\\/](?:Workbench|AgentWorkbenchBody)\.tsx$/,
  },
  BASELINE_DESKTOP_GROUPS[2]!,
  {
    name: "chat-ui",
    priority: 1,
    includeDependenciesRecursively: false,
    test: /src[\\/]features[\\/]chat[\\/]acp[\\/](?!(?:AcpChatPresentation|MermaidFigure|AcpChatProjection|AcpAnnouncementRegions))[^\\/]+\.tsx$/,
  },
  ...DESKTOP_GROUPS.filter((group) =>
    [
      "markdown-rendering",
      "diff-rendering",
      "snapshotDiff",
      "agent-diff-model",
    ].includes(group.name),
  ).map((group) =>
    group.name === "snapshotDiff"
      ? {
          ...group,
          test: /src[\\/]features[\\/]diff[\\/]snapshotDiff\.ts$/,
        }
      : group,
  ),
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

const COMPOSER_MENU_DYNAMIC_FACADES = [
  "src/features/chat/ComposerAccessMenu.tsx",
  "src/features/chat/ComposerEffortMenu.tsx",
  "src/features/chat/ComposerModelMenu.tsx",
];
const REQUEST_ONLY_DYNAMIC_FACADES = [
  "src/workbench/DirtyTabDialog.tsx",
  "src/ports/editorWatchSession.ts",
  "src/features/files/newFileResult.ts",
  "src/app/browserPopupNative.ts",
  "src/app/agentWorkingDiffRepository.ts",
  "src/features/files/fileRenameResult.ts",
  "src/features/files/resourceTrashContract.ts",
  "src/features/browser/browserSuggestions.ts",
  "src/features/browser/browserClipboard.ts",
  "src/features/browser/BrowserFindControls.tsx",
  "src/features/browser/BrowserAnnotatePopover.tsx",
  "src/features/browser/ClosedBrowserSurface.tsx",
  "src/features/browser/attachBrowserPageContext.ts",
  "src/features/files/fileSymbolNavigation.ts",
  "src/features/files/FileGoToLineBar.tsx",
];
const REQUIRED_DYNAMIC_FACADES = [
  "src/features/chat/ComposerAddActions.tsx",
  ...REQUEST_ONLY_DYNAMIC_FACADES,
  ...COMPOSER_MENU_DYNAMIC_FACADES,
  ...Object.values(CODEMIRROR_LANGUAGE_DYNAMIC_FACADES),
  "node_modules/@tauri-apps/api/core.js",
  "node_modules/@tauri-apps/api/event.js",
  // Releasing an open File's tabs for a move (ED-16) loads with that move.
  "src/app/openFileRelease.ts",
  // Explorer's files.exclude matcher loads beside the startup bundle.
  "src/domain/pathGlobMatcher.ts",
  // The device toolbar, its state, handles and styles load when first shown.
  "src/features/browser/BrowserDeviceController.tsx",
  // The browser downloads popover body loads on its first open.
  "src/features/browser/BrowserDownloadsPanel.tsx",
  // Annotate's composer chips and hover card load with the first element.
  "src/features/browser/BrowserElementChips.tsx",
  // The History page loads on its first open (BF-7c).
  "src/features/browser/BrowserHistoryPage.tsx",
  // The Agent New tab page and its chat resource picker load on first use.
  "src/features/browser/BrowserNewTab.tsx",
  "src/features/browser/BrowserSurface.tsx",
  "src/features/chat/ChatHandoffDialog.tsx",
  "src/features/chat/ChatNavigation.tsx",
  "src/features/chat/ChatQueueExecution.tsx",
  "src/features/chat/ChatQueueRuntime.tsx",
  // Recovery notices and sign-in actions stay off the ready Chat path.
  "src/features/chat/HarnessDiscoveryNotice.tsx",
  "src/features/chat/ClaudeAuthActions.tsx",
  // Queued-message rows load with the first queued message.
  "src/features/chat/ComposerQueuedPrompts.tsx",
  "src/features/chat/ForkChat.tsx",
  "src/features/chat/TranscriptInteractions.tsx",
  "src/features/chat/acp/MermaidFigure.tsx",
  // The pending-approval card loads only when a request awaits a decision.
  "src/features/chat/acp/PermissionPrompt.tsx",
  "src/features/chat/acp/TurnFileActivity.tsx",
  // The stream-fade run tracker loads beside the transcript, off its path.
  "src/features/chat/acp/streamTail.ts",
  // A terminal turn's Git workspace changes load after the turn settles.
  "src/features/chat/acp/turnWorkspaceChanges.ts",
  "src/features/chat/acp/TurnEditSummary.tsx",
  "src/features/harness/HarnessSignInStatus.tsx",
  // The Chat search dialog loads on the first Search chats request.
  "src/features/chatSearch/ChatSearchControl.tsx",
  "src/features/chat/chatImageTransfer.ts",
  "src/features/diff/DiffSurface.tsx",
  "src/features/diff/ChangesSurface.tsx",
  "src/features/diff/ChangesTab.tsx",
  // The Find and Replace bar (EP-04) loads on the first Find or Replace.
  "src/features/files/FileFindBar.tsx",
  // Explorer loads on first show, off the Agent startup path.
  "src/features/files/ExplorerSurface.tsx",
  "src/features/files/FileSurface.tsx",
  // Harness sign-in status loads when the desktop picker has harnesses.
  "src/features/harness/harnessSignIn.ts",
  "src/features/files/MixedEolFileView.tsx",
  // New File hands focus to the editor through this, off the startup path.
  "src/features/files/editorFocusRequest.ts",
  // The editing features (EP-03) load with the first editable File.
  "src/features/files/editorEditing.ts",
  // Git marks (EP-13) load only while on and a repository is open.
  "src/features/git/gitDecorationsUi.ts",
  // The Editor tab strip loads with the Editor.
  "src/workbench/TabStrip.tsx",
  // Change markers (EP-14) load with the first File in a repository.
  "src/features/files/gitGutter/index.ts",
  // Outline is an Editor tool panel; its syntax parse loads with CodeMirror.
  "src/features/outline/OutlineSurface.tsx",
  "src/features/outline/syntaxOutline.ts",
  "src/features/preview/AgentPreviewPresentation.tsx",
  "src/features/preview/PreviewSurface.tsx",
  // Quick Open (EP-08) loads on its first open, or while idle.
  "src/features/quickOpen/QuickOpen.tsx",
  "src/features/quickOpen/QuickOpenHost.tsx",
  "src/features/search/SearchSurface.tsx",
  "src/features/settings/AgentBrowserSettings.tsx",
  "src/features/settings/AgentKeyboardSettings.tsx",
  "src/features/settings/AgentSettingsExecution.tsx",
  "src/features/settings/AgentSettingsSurface.tsx",
  "src/features/settings/openEditorSettings.ts",
  "src/workbench/ActiveEditorCanvas.tsx",
  // The sidebar options menu loads on its first open.
  "src/workbench/AgentRailOptionsPopover.tsx",
  // The Create project dialog loads on the rail's first "+".
  "src/workbench/CreateProjectDialog.tsx",
  "src/workbench/AgentTaskResources.tsx",
  "src/workbench/AgentArtifactHeader.tsx",
  "src/workbench/AgentArtifactOpenMenu.tsx",
  "src/workbench/AgentArtifactTabStrip.tsx",
  "src/workbench/AgentChatSplitter.tsx",
  "src/workbench/AgentCollapsedNavigation.tsx",
  "src/workbench/AgentResourcesPopover.tsx",
  "src/workbench/AgentSideChatHeaders.tsx",
  "src/workbench/AgentProjectMenu.tsx",
  "src/workbench/AgentRailArchiveNotice.tsx",
  "src/workbench/AgentRecentChats.tsx",
  "src/workbench/useAgentRailStatus.ts",
  "src/workbench/ProjectChatActionPopover.tsx",
  "src/workbench/ProjectChatRename.tsx",
  "src/workbench/SettingsSurface.tsx",
  // The Browser settings page remains a lazy facade.
  "src/workbench/settings/SettingsBrowserPage.tsx",
  "src/workbench/SourceControlView.tsx",
  "src/workbench/SubagentSurface.tsx",
  "src/workbench/TerminalDock.tsx",
  "src/workbench/TerminalDockShell.tsx",
  // Terminal controls load outside the shared workbench.
  "src/workbench/terminalDockCommands.tsx",
].sort();

const ORDINARY_DYNAMIC_FACADES = [
  ...REQUIRED_DYNAMIC_FACADES,
  "src/app/agentDiffAdmission.ts",
  "src/app/agentDiffLoad.ts",
  "src/features/chat/acp/AcpChatPresentation.tsx",
  "src/features/chat/acp/InlineSavedDiff.tsx",
  "src/services/acpFileSnapshot.ts",
  "src/features/files/CodeMirrorEditorRuntime.ts",
  "src/workbench/AgentWorkbenchBody.tsx",
  "src/workbench/Workbench.tsx",
].sort();

const CONSOLIDATED_AGENT_MODULES = [
  "src/app/subagentPaneStore.ts",
  "src/features/chat/acp/chatLiveSnapshot.ts",
  "src/features/chat/acp/useAcpChatLiveSnapshot.ts",
  "src/features/chat/composerChrome.ts",
  "src/features/preview/AgentPreviewSurface.tsx",
  "src/features/preview/previewDesktop.ts",
  "src/features/preview/previewPresentation.ts",
  "src/features/preview/previewProjectionStore.ts",
  "src/features/preview/useHarnessBrowseSession.ts",
  "src/workbench/AgentArtifactCollapsedPane.tsx",
  "src/workbench/AgentArtifactFocusGuard.tsx",
  "src/workbench/AgentArtifactPane.tsx",
  "src/workbench/agentArtifactTabPresentation.ts",
  "src/workbench/AgentChatPanes.tsx",
  "src/workbench/agentPreviewControllerOwner.ts",
  "src/workbench/AgentRailOptionsMenu.tsx",
  "src/workbench/AgentRailSections.tsx",
  "src/workbench/agentRailViewPrefs.ts",
  "src/workbench/AgentTaskHeader.tsx",
  "src/workbench/agentTaskHeaderLazy.ts",
  "src/workbench/AgentWorkbenchBody.tsx",
  "src/workbench/agentChatRail.ts",
  "src/workbench/agentRailStatusView.ts",
  "src/workbench/artifactChipStats.ts",
  "src/workbench/LeftSidebar.tsx",
  "src/workbench/ProjectChatActionMenu.tsx",
  "src/workbench/ProjectChatNavigation.tsx",
  "src/workbench/useAgentArtifactFollowLifecycle.ts",
  "src/workbench/useAgentArtifactPaneFocus.ts",
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

function setDesktopGraphGroups(
  configured: InlineConfig,
  groups?: typeof DESKTOP_GROUPS,
) {
  const output = configured.build?.rolldownOptions?.output;
  if (!output || Array.isArray(output))
    throw new Error("Expected one desktop output configuration");
  output.codeSplitting = groups ? { groups } : undefined;
}

function desktopGraphConfig(withInitialGroup: boolean): InlineConfig {
  const configured = initialGroupConfig("build", DESKTOP_NATIVE_BUILD_MODE);
  const buildConfig = { ...configured.build };
  if (!withInitialGroup) setDesktopGraphGroups(configured);
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

function legacyChunkGroupConfig(): InlineConfig {
  const configured = desktopGraphConfig(true);
  setDesktopGraphGroups(configured, LANE_BASE_DESKTOP_GROUPS);
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
    chunkNames: new Map(chunks.map((chunk) => [chunk.fileName, chunk.name])),
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

function expectNoNewFeatureModules(
  graph: ReturnType<typeof graphInventory>,
  baseline: ReturnType<typeof graphInventory>,
  facade: string,
  additionalBaseline: readonly string[] = [],
) {
  const allowed = new Set([
    ...closureForModule(baseline, facade),
    ...additionalBaseline,
  ]);
  expect(
    closureForModule(graph, facade).filter(
      (id) =>
        !allowed.has(id) && !id.endsWith("/src/app/snapshotProjection.ts"),
    ),
    facade,
  ).toEqual([]);
}

describe("desktop-native runtime resolver", () => {
  it("loads transcript decoders with Chat rather than harness discovery", async () => {
    const graph = graphInventory(await build(desktopGraphConfig(true)));
    for (const module of [
      "/src/services/acpDesktopResponseTurns.ts",
      "/src/services/acpDesktopResponseItems.ts",
      "/src/services/acpDesktopResponsePayloads.ts",
      "/src/services/acpDesktopPreview.ts",
      "/src/services/acpDesktopPermission.ts",
    ]) {
      expect(
        graph.initialModules.some((id) => id.endsWith(module)),
        module,
      ).toBe(false);
      expect(
        closureForModule(
          graph,
          "src/features/chat/acp/AcpChatPresentation.tsx",
        ).some((id) => id.endsWith(module)),
        module,
      ).toBe(true);
    }
  }, 90_000);

  it("keeps Agent-only rail and composer logic out of Editor closures", async () => {
    const graph = graphInventory(await build(desktopGraphConfig(true)));
    for (const facade of [
      "src/workbench/Workbench.tsx",
      "src/features/files/FileSurface.tsx",
      "src/features/diff/DiffSurface.tsx",
      "src/features/search/SearchSurface.tsx",
      "src/features/files/CodeMirrorEditorRuntime.ts",
    ]) {
      for (const module of [
        "/src/features/chat/composerChrome.ts",
        "/src/workbench/agentChatRail.ts",
        "/src/workbench/agentRailStatusView.ts",
        "/src/ui/IconButton.tsx",
        "/src/ui/RetainedActivity.tsx",
      ]) {
        expect(
          closureForModule(graph, facade).some((id) => id.endsWith(module)),
          `${facade}: ${module}`,
        ).toBe(false);
      }
    }
  }, 90_000);

  it("keeps the Editor tab strip and its Git decorations (EP-13) out of the Agent first-Chat closure", async () => {
    const graph = graphInventory(await build(desktopGraphConfig(true)));
    for (const facade of [
      "src/workbench/Workbench.tsx",
      "src/workbench/AgentWorkbenchBody.tsx",
      "src/features/chat/ChatSurface.tsx",
      "src/features/chat/acp/AcpChatPresentation.tsx",
    ]) {
      for (const module of [
        "/src/features/git/gitDecorations.ts",
        "/src/features/git/GitDecorationBadge.tsx",
        "/src/features/git/GitTabTitle.tsx",
        "/src/workbench/TabStrip.tsx",
        "/src/features/git/useRootGitDecorations.ts",
        "/src/features/git/git-decorations.css",
      ]) {
        expect(
          closureForModule(graph, facade).some((id) => id.endsWith(module)),
          `${facade}: ${module}`,
        ).toBe(false);
      }
    }
  }, 90_000);

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
  it("ignores generated work and evidence paths without ignoring application source", () => {
    const config = initialGroupConfig("serve", DESKTOP_NATIVE_BUILD_MODE);
    const ignored = config.server?.watch?.ignored;
    const ignores = (file: string) =>
      (Array.isArray(ignored) ? ignored : [ignored]).some((matcher) =>
        typeof matcher === "function"
          ? matcher(file)
          : typeof matcher === "string"
            ? path.matchesGlob(file, matcher)
            : matcher instanceof RegExp && matcher.test(file),
      );
    for (const generated of [
      "work/landing-tmp/fixture/tsconfig.json",
      "work/landing-tmp/fixture/index.html",
      "output/ui-results/index.html",
      "local-evidence/receipt.json",
      "src-tauri/src/lib.rs",
    ]) {
      expect(ignores(path.join(root, generated)), generated).toBe(true);
    }
    for (const source of [
      "index.html",
      "tsconfig.json",
      "src/app/store.ts",
      "src/features/work/Surface.tsx",
      "tests/ui/fixtures/visualScenarios.ts",
    ]) {
      expect(ignores(path.join(root, source)), source).toBe(false);
    }
  });

  it("keeps chat announcements, edit summaries and queued-tray sizing out of Editor closures", async () => {
    const graph = graphInventory(await build(desktopGraphConfig(true)));
    const chatOnly = [
      "src/features/chat/ChatSurface.tsx",
      "src/features/chat/ComposerQueuedPrompts.tsx",
      "src/features/chat/acp/AcpAnnouncementRegions.tsx",
      "src/features/chat/acp/agentEditSummary.ts",
      "src/features/chat/acp/chatEditSummary.ts",
      "src/features/chat/acp/turnWorkspaceBridge.ts",
      "src/features/chat/acp/editingFiles.ts",
    ];
    for (const facade of [
      "src/workbench/Workbench.tsx",
      "src/features/diff/DiffSurface.tsx",
      "src/features/search/SearchSurface.tsx",
      "src/features/files/FileSurface.tsx",
      "src/features/files/CodeMirrorEditorRuntime.ts",
    ]) {
      const closure = closureForModule(graph, facade);
      for (const module of chatOnly)
        expect(
          closure.some((id) => id.endsWith(module)),
          `${facade}: ${module}`,
        ).toBe(false);
    }
    const chat = closureForModule(graph, "src/features/chat/ChatSurface.tsx");
    for (const module of COMPOSER_MENU_DYNAMIC_FACADES) {
      expect(graph.dynamicFacades).toContain(module);
      expect(
        chat.some((id) => id.endsWith(module)),
        module,
      ).toBe(false);
      expect(
        graph.initialModules.some((id) => id.endsWith(module)),
        module,
      ).toBe(false);
    }
    // chatEditSummary and the lazy queued-message rows are not static.
    for (const module of chatOnly.filter(
      (id) =>
        !id.endsWith("chatEditSummary.ts") &&
        !id.endsWith("ComposerQueuedPrompts.tsx"),
    ))
      expect(
        chat.some((id) => id.endsWith(module)),
        module,
      ).toBe(true);
  }, 90_000);

  it("keeps the Agent-body reserve separate from the Editor shell", () => {
    expect(DESKTOP_GROUPS[0]).toEqual(BASELINE_DESKTOP_GROUPS[0]);
    const workbench = DESKTOP_GROUPS.find(
      (group) => group.name === "workbench-deps",
    );
    const agent = DESKTOP_GROUPS.find(
      (group) => group.name === "agent-workbench",
    );
    expect(workbench?.priority).toBe(1);
    if (!(workbench?.test instanceof RegExp))
      throw new Error("Expected a bounded Workbench filter");
    if (!(agent?.test instanceof RegExp))
      throw new Error("Expected a bounded Agent filter");
    for (const separator of ["/", "\\"]) {
      const id = (module: string) =>
        `${root}/src/workbench/${module}.tsx`.replaceAll("/", separator);
      expect(workbench.test.test(id("Workbench"))).toBe(true);
      expect(workbench.test.test(id("AgentWorkbenchBody"))).toBe(false);
      expect(agent.test.test(id("AgentWorkbenchBody"))).toBe(true);
      expect(agent.test.test(id("Workbench"))).toBe(false);
      for (const module of [
        "SubagentSurface",
        "SettingsSurface",
        "SourceControlView",
        "TerminalDock",
        "TerminalDockShell",
      ]) {
        expect(workbench.test.test(id(module)), module).toBe(false);
        expect(agent.test.test(id(module)), module).toBe(false);
      }
    }
  });

  it("preserves chat-helper ownership without adding feature modules to lane-base closures", async () => {
    const baselineConfig = desktopGraphConfig(true);
    setDesktopGraphGroups(baselineConfig, LANE_BASE_DESKTOP_GROUPS);
    const [baseline, grouped] = await Promise.all([
      build(baselineConfig),
      build(desktopGraphConfig(true)),
    ]);
    const baselineGraph = graphInventory(baseline);
    const groupedGraph = graphInventory(grouped);
    const files = [
      "src/features/chat/acp/TurnActivityItem.tsx",
      "src/features/chat/acp/PrimaryActivities.tsx",
      "src/features/chat/acp/ChatCodeBlock.tsx",
    ].map((id) => groupedGraph.moduleFiles.get(id));
    expect(files.every((file) => typeof file === "string")).toBe(true);
    expect(new Set(files).size).toBe(1);
    expect(groupedGraph.chunkNames.get(files[0]!)).toBe("agent-workbench");
    for (const facade of [
      "src/workbench/AgentWorkbenchBody.tsx",
      "src/features/files/CodeMirrorEditorRuntime.ts",
      "src/features/files/FileSurface.tsx",
      "src/features/diff/DiffSurface.tsx",
    ]) {
      expectNoNewFeatureModules(
        groupedGraph,
        baselineGraph,
        facade,
        facade === "src/workbench/AgentWorkbenchBody.tsx"
          ? closureForModule(
              baselineGraph,
              "src/features/chat/acp/AcpChatPresentation.tsx",
            )
          : [],
      );
    }
  }, 90_000);

  it("preserves the legacy module graph while separating Agent and Editor closures", async () => {
    const baselineConfig = legacyChunkGroupConfig();
    const [baseline, grouped] = await Promise.all([
      build(baselineConfig),
      build(desktopGraphConfig(true)),
    ]);
    const baselineGraph = graphInventory(baseline);
    const groupedGraph = graphInventory(grouped);
    const workbench = "src/workbench/Workbench.tsx";
    const agent = "src/workbench/AgentWorkbenchBody.tsx";
    const sharedFile = groupedGraph.moduleFiles.get(workbench);
    expect(groupedGraph.chunkNames.get(sharedFile!)).toBe("workbench-deps");
    expect(groupedGraph.moduleFiles.get("src/ui/nativePageLayers.ts")).toBe(
      sharedFile,
    );
    const agentFile = groupedGraph.moduleFiles.get(agent);
    expect(groupedGraph.chunkNames.get(agentFile!)).toBe("agent-workbench");
    expect(agentFile).not.toBe(sharedFile);
    expect(
      groupedGraph.moduleFiles.get("src/features/chat/ChatSurface.tsx"),
    ).toBe(agentFile);
    for (const module of CONSOLIDATED_AGENT_MODULES) {
      expect(groupedGraph.moduleFiles.get(module), module).toBe(agentFile);
    }
    const artifactStrip = groupedGraph.moduleFiles.get(
      "src/workbench/AgentArtifactTabStrip.tsx",
    );
    expect(artifactStrip).toBeDefined();
    expect(artifactStrip).not.toBe(agentFile);
    expect(artifactStrip).not.toBe(sharedFile);
    const resources = "src/workbench/AgentTaskResources.tsx";
    const resourcesFile = groupedGraph.moduleFiles.get(resources);
    expect(groupedGraph.chunkNames.get(resourcesFile!)).toBe(
      "AgentTaskResources",
    );
    expect(resourcesFile).not.toBe(agentFile);
    expect(resourcesFile).not.toBe(sharedFile);
    for (const facade of [workbench, agent]) {
      for (const module of [
        resources,
        "src/workbench/AgentResourcesPopover.tsx",
      ]) {
        expect(closureForModule(groupedGraph, facade)).not.toContain(
          path.join(root, module).split(path.sep).join("/"),
        );
      }
    }
    // Chat search (dialog, fan-out, domain, native adapter) is one lazy chunk.
    const searchFile = groupedGraph.moduleFiles.get(
      "src/features/chatSearch/ChatSearchControl.tsx",
    );
    for (const module of [
      "src/features/chatSearch/chatSearchDesktop.ts",
      "src/features/chatSearch/chatSearchDomain.ts",
      "src/features/chatSearch/chatSearchFanout.ts",
    ]) {
      expect(groupedGraph.moduleFiles.get(module), module).toBe(searchFile);
    }
    expect(searchFile).not.toBe(agentFile);
    expect(searchFile).not.toBe(sharedFile);
    // The Hand off dialog loads on the first Hand off; the trigger stays eager.
    const handoffFile = groupedGraph.moduleFiles.get(
      "src/features/chat/ChatHandoffDialog.tsx",
    );
    expect(handoffFile).toBeDefined();
    expect(handoffFile).not.toBe(agentFile);
    expect(handoffFile).not.toBe(sharedFile);
    expect(
      groupedGraph.moduleFiles.get("src/features/chat/ChatHandoffControl.tsx"),
    ).toBe(agentFile);
    expect(groupedGraph.allModules).toEqual(baselineGraph.allModules);
    expect(groupedGraph.initialModules).toEqual(baselineGraph.initialModules);
    expect(new Set(groupedGraph.allModules).size).toBe(
      groupedGraph.allModules.length,
    );
    const subagent = "src/workbench/SubagentSurface.tsx";
    const childFile = groupedGraph.moduleFiles.get(subagent);
    expect(groupedGraph.chunkNames.get(childFile!)).toBe("SubagentSurface");
    expect(childFile).not.toBe(sharedFile);
    // Browser subscriptions load native APIs on demand, outside UI closures.
    for (const facade of [
      workbench,
      agent,
      "src/features/browser/BrowserSurface.tsx",
    ]) {
      expect(
        closureForModule(groupedGraph, facade).filter((id) =>
          /node_modules\/@tauri-apps\/api\/(?:core|event)\.js$/.test(id),
        ),
        facade,
      ).toEqual([]);
    }
    expect(groupedGraph.dynamicFacades).toEqual(REQUIRED_DYNAMIC_FACADES);
    for (const facade of [
      workbench,
      agent,
      "src/features/files/CodeMirrorEditorRuntime.ts",
      "src/features/files/FileSurface.tsx",
      "src/features/diff/DiffSurface.tsx",
      "src/features/search/SearchSurface.tsx",
    ]) {
      expectNoNewFeatureModules(
        groupedGraph,
        baselineGraph,
        facade,
        facade === agent
          ? closureForModule(
              baselineGraph,
              "src/features/chat/acp/AcpChatPresentation.tsx",
            )
          : [],
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
      expectNoNewFeatureModules(
        groupedGraph,
        baselineGraph,
        facade,
        closureForModule(
          baselineGraph,
          "src/features/files/CodeMirrorEditorRuntime.ts",
        ),
      );
    }
    const presentation = "src/features/chat/acp/AcpChatPresentation.tsx";
    const presentationFile = groupedGraph.moduleFiles.get(presentation);
    expect(presentationFile).toBe(agentFile);
    expect(groupedGraph.chunkNames.get(presentationFile!)).toBe(
      "agent-workbench",
    );
    expect(closureForModule(groupedGraph, presentation)).toContain(
      path
        .join(root, "src/features/chat/acp/ToolItemDetails.tsx")
        .split(path.sep)
        .join("/"),
    );
    for (const facade of [workbench, agent, presentation]) {
      expect(
        closureForModule(groupedGraph, facade).filter((id) =>
          /(?:TurnEditSummary\.tsx|turnWorkspaceChanges\.ts|HarnessSignInStatus\.tsx|harnessSignIn(?:Bridge)?\.ts|InlineSavedDiff\.tsx|DiffHunks\.tsx|snapshotDiff\.ts|MermaidFigure\.tsx|mermaid[^/]+\.ts|@codemirror|CodeMirrorEditorRuntime\.ts)/.test(
            id,
          ),
        ),
        facade,
      ).toEqual([]);
      expect(
        closureForModule(groupedGraph, facade).some((id) =>
          id.endsWith("src/features/chat/acp/turnWorkspaceBridge.ts"),
        ),
        facade,
      ).toBe(facade !== workbench);
    }
    for (const facade of [
      workbench,
      agent,
      "src/features/files/FileSurface.tsx",
      "src/features/diff/DiffSurface.tsx",
      "src/features/search/SearchSurface.tsx",
      "src/features/files/CodeMirrorEditorRuntime.ts",
    ]) {
      expect(
        closureForModule(groupedGraph, facade).some((id) =>
          id.endsWith(presentation),
        ),
        facade,
      ).toBe(facade === agent);
    }
    const snapshotFile = groupedGraph.moduleFiles.get(
      "src/features/diff/snapshotDiff.ts",
    );
    expect(groupedGraph.chunkNames.get(snapshotFile!)).toBe("snapshotDiff");
    for (const module of [
      "src/app/agentDiffAdmission.ts",
      "src/app/agentDiffLoad.ts",
      "src/app/snapshotTabPresence.ts",
      "src/services/acpFileSnapshot.ts",
      "src/features/chat/acp/InlineSavedDiff.tsx",
    ]) {
      expect(groupedGraph.moduleFiles.get(module), module).toBe(snapshotFile);
      for (const facade of [
        workbench,
        agent,
        "src/features/diff/DiffSurface.tsx",
        "src/features/search/SearchSurface.tsx",
        "src/features/files/FileSurface.tsx",
        "src/features/files/CodeMirrorEditorRuntime.ts",
      ]) {
        expect(
          closureForModule(groupedGraph, facade).some((id) =>
            id.endsWith(module),
          ),
          `${facade}: ${module}`,
        ).toBe(false);
      }
    }
    const runtimeFile = groupedGraph.moduleFiles.get(
      "src/features/files/CodeMirrorEditorRuntime.ts",
    );
    expect(groupedGraph.chunkNames.get(runtimeFile!)).toBe("codemirror-deps");
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
    expect(desktopBuild.build.rolldownOptions?.optimization).toEqual({
      inlineConst: true,
    });
    expect(desktopBuild.build.rolldownOptions?.output).toMatchObject({
      entryFileNames: "assets/[hash].js",
      chunkFileNames: "assets/[hash].js",
    });
    expect(desktopServe.build?.rolldownOptions).toBeUndefined();
    expect(browserBuild.build?.rolldownOptions).toBeUndefined();
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
      ...REQUEST_ONLY_DYNAMIC_FACADES.map((file) => `/${file}`),
      "/src/domain/agentDiff.ts",
      "/src/services/acpFileSnapshot.ts",
      "/src/app/agentDiffAdmission.ts",
      "/src/app/agentDiffLoad.ts",
      "/src/features/diff/snapshotDiff.ts",
      "/src/workbench/AgentWorkbenchBody.tsx",
      "/src/workbench/AgentTaskResources.tsx",
      "/src/workbench/AgentResourcesPopover.tsx",
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
      closureForModule(
        ordinaryGraph,
        "src/workbench/AgentWorkbenchBody.tsx",
      )?.some((id) =>
        id.endsWith("/src/features/settings/agentSettingsState.ts"),
      ),
    ).toBe(true);
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

    const editorRuntimeClosure = closureForModule(
      groupedGraph,
      "src/features/files/CodeMirrorEditorRuntime.ts",
    );
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

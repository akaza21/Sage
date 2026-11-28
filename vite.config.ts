import react from "@vitejs/plugin-react";
import { configDefaults, defineConfig } from "vitest/config";
import path from "node:path";
import { fileURLToPath } from "node:url";
import type { Plugin } from "vite";
import type { OutputOptions } from "rolldown";
import { htmlCompletionSchemaPlugin } from "./scripts/htmlCompletionSchema";
import { desktopReactRuntimeProjectionPlugin } from "./scripts/reactRuntimeProjection";

const host = process.env.TAURI_DEV_HOST;
const sourceMaps = process.env.SAGE_BUILD_SOURCEMAPS === "1";
const root = path.dirname(fileURLToPath(import.meta.url));
const workbenchRuntime = path.join(root, "src/ports/workbenchRuntime.ts");
const workbenchRuntimeTest = path.join(
  root,
  "src/ports/workbenchRuntime.test.ts",
);
const simulatorWorkbenchRuntime = path.join(
  root,
  "tests/ui/simulator/workbenchRuntime.ts",
);
const devWatchIgnoredRoots = new Set([
  "src-tauri",
  "target",
  "work",
  "output",
  "local-evidence",
]);

export const DESKTOP_NATIVE_BUILD_MODE = "desktop-native";
export const UI_FIXTURE_MODE = "ui-fixture";

/**
 * Keep startup dependencies and bounded editor-core dependencies separate.
 * Language packages and feature implementations stay on their own demand paths.
 */
type DesktopCodeSplitting = Exclude<
  OutputOptions["codeSplitting"],
  boolean | undefined
>;

export const DESKTOP_NATIVE_CODE_SPLITTING_GROUPS: NonNullable<
  DesktopCodeSplitting["groups"]
> = [
  { name: "initial-deps", tags: ["$initial"], priority: 2 },
  {
    name: "workbench-deps",
    priority: 1,
    // Shared page-layer state and the shared Notice belong to the shell, not
    // Agent UI, so Editor notices never load the Agent chunk.
    test: /src[\\/](?:(?:workbench[\\/]Workbench|ui[\\/](?:Notice|ResourceStateNotice|SearchablePicker))\.tsx|(?:app[\\/]snapshotProjection|ui[\\/]nativePageLayers)\.ts)$/,
  },
  {
    name: "modal-dialog",
    priority: 2,
    includeDependenciesRecursively: false,
    // Shared confirmations must not pull Agent UI into an Editor resource.
    test: /src[\\/]ui[\\/]ModalDialog\.tsx$/,
  },
  {
    name: "menu-popover",
    priority: 2,
    includeDependenciesRecursively: false,
    // The shared menu loads with an Editor's own menus and with an Agent
    // menu's first open, never with the Agent UI behind that menu. A
    // searchable picker's open list and its rows join it: they load only
    // where a MenuPopover opens, so one chunk saves a request.
    test: /src[\\/]ui[\\/](?:(?:MenuPopover|SearchablePickerPopover|PickerOption)\.tsx|menuPopover(?:Position|DialogFocus)\.ts)$/,
  },
  {
    name: "file-kind-icons",
    priority: 2,
    includeDependenciesRecursively: false,
    // Resource icons are shared by the deferred Agent and Editor tab strips.
    test: /src[\\/]features[\\/]files[\\/](?:FileKindIcon\.tsx|fileKind\.ts)$/,
  },
  {
    name: "agent-workbench",
    priority: 1,
    test: /src[\\/](?:workbench[\\/]AgentWorkbenchBody\.tsx$|features[\\/]chat[\\/]acp[\\/](?!(?:ChatMarkdown|ExtensionMessage|MermaidFigure|mermaid|AcpChatProjection|AcpAnnouncementRegions|InlineSavedDiff|PermissionPrompt|UserInputPrompt|UserInputField|SessionNoticeStrip|approvalCard|TurnFileActivity|streamTail|turnWorkspace|TurnEditSummary|BackgroundTerminal|chatEditSummary|useGoalMutation|goalMutability|SessionSurfacePanel|useSessionControl|ComposerChrome|sessionChrome))[^\\/]+\.(?:tsx|ts)$)/,
  },
  {
    name: "chat-setup",
    priority: 2,
    includeDependenciesRecursively: false,
    // Draft navigation, a chat's access and effort choices, the chat creation
    // request and the Editor chat's session header form one deferred feature.
    // Sharing its tiny controllers avoids request/import overhead while
    // keeping all of them outside Agent and Editor startup. First-send
    // project ownership and handoff retain their own bounded dynamic entry.
    test: /src[\\/](?:ports[\\/]chatAccessChanges\.ts|app[\\/](?:newChatDraft(?:Actions|Navigation)|runCreateChat|sessionAccessActions|chat(?:AccessDrafts|EffortDraft))\.ts|features[\\/]chat[\\/](?:(?:chatAccess(?:Mode|Icons|Choice)|endedSession)\.ts|(?:ChatAccess(?:Draft)?(?:Selection|View)|NewChatDraftSurface|DraftEffortControl|ChatSessionHeader|SessionWorkingPill)\.tsx))$/,
  },
  {
    name: "codemirror-deps",
    priority: 1,
    test: /(?:node_modules[\\/]@codemirror[\\/](?:state|view|language|commands|autocomplete)[\\/]|src[\\/]features[\\/]files[\\/]CodeMirrorEditorRuntime\.ts$)/,
  },
  {
    name: "markdown-rendering",
    priority: 2,
    includeDependenciesRecursively: false,
    test: /(?:node_modules[\\/](?:markdown-it|argparse|entities|linkify-it|mdurl|punycode.js|uc.micro)[\\/]|src[\\/]features[\\/]files[\\/](?:MarkdownBlocks|markdownPreview|markdownTokens|markdownStream|markdownLimits|markdownContainers|markdownTextLocations)\.tsx?$|src[\\/]features[\\/]chat[\\/]acp[\\/](?:MermaidFigureBoundary|ExtensionMessage)\.tsx$)/,
  },
  {
    name: "diff-rendering",
    priority: 2,
    includeDependenciesRecursively: false,
    test: /src[\\/]features[\\/]diff[\\/]DiffHunks\.tsx$/,
  },
  {
    name: "snapshotDiff",
    priority: 2,
    includeDependenciesRecursively: false,
    test: /src[\\/](?:(?:features[\\/]diff[\\/]snapshotDiff|app[\\/](?:agentDiff(?:Admission|Load)|snapshotTabPresence)|services[\\/]acpFileSnapshot)\.ts|features[\\/]chat[\\/]acp[\\/]InlineSavedDiff\.tsx)$/,
  },
  {
    name: "agent-diff-model",
    priority: 2,
    includeDependenciesRecursively: false,
    test: /(?:src[\\/]domain[\\/]agentDiff\.ts$|src[\\/]features[\\/]chat[\\/]acp[\\/]agentEditSummary\.ts$)/,
  },
];

// The browser simulator is a test fixture. Only these mode/command pairs swap
// the app's runtime import for it: Playwright's dev server, the browser
// performance build, and Vitest. The production runtime's own test keeps the
// real module.
const SIMULATOR_MODES: Readonly<Record<string, string>> = {
  [UI_FIXTURE_MODE]: "serve",
  "performance-browser": "build",
  test: "serve",
};

export function resolveSimulatorWorkbenchRuntime(
  command: string,
  mode: string,
  source: string,
  importer?: string,
): string | undefined {
  if (SIMULATOR_MODES[mode] !== command) return undefined;
  const from = importer && path.resolve(importer.split("?")[0]!);
  if (from === workbenchRuntimeTest) return undefined;
  const resolved = source.startsWith("/src/")
    ? path.join(root, source)
    : source.startsWith(".") && from
      ? path.resolve(path.dirname(from), source)
      : path.resolve(source);
  if (resolved === workbenchRuntime || `${resolved}.ts` === workbenchRuntime) {
    return simulatorWorkbenchRuntime;
  }
  return undefined;
}

export function simulatorWorkbenchRuntimePlugin(
  command: string,
  mode: string,
): Plugin | undefined {
  if (SIMULATOR_MODES[mode] !== command) return undefined;
  return {
    name: "sage-simulator-workbench-runtime",
    enforce: "pre",
    resolveId(source, importer) {
      return resolveSimulatorWorkbenchRuntime(command, mode, source, importer);
    },
  };
}

export function desktopNativeBuildTargetPlugin(
  command: string,
  mode: string,
): Plugin | undefined {
  if (command !== "build" || mode !== DESKTOP_NATIVE_BUILD_MODE) {
    return undefined;
  }
  return {
    name: "sage-desktop-native-build-target",
    generateBundle() {
      this.emitFile({
        type: "asset",
        fileName: ".sage-web-build-target.json",
        source: `${JSON.stringify({ schemaVersion: 1, target: DESKTOP_NATIVE_BUILD_MODE })}\n`,
      });
    },
  };
}

export default defineConfig(({ command, mode }) => ({
  cacheDir: process.env.SAGE_VITE_CACHE_DIR,
  plugins: [
    react(),
    desktopReactRuntimeProjectionPlugin(command, mode),
    desktopNativeBuildTargetPlugin(command, mode),
    simulatorWorkbenchRuntimePlugin(command, mode),
    htmlCompletionSchemaPlugin(),
  ],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host ?? "127.0.0.1",
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      ignored: (file: string) => {
        const directory = path.relative(root, file).split(path.sep)[0] ?? "";
        return devWatchIgnoredRoots.has(
          process.platform === "win32" ? directory.toLowerCase() : directory,
        );
      },
    },
  },
  build: {
    manifest: true,
    modulePreload: false,
    target: "es2022",
    sourcemap: sourceMaps,
    reportCompressedSize: true,
    ...(command === "build" && mode === DESKTOP_NATIVE_BUILD_MODE
      ? {
          rolldownOptions: {
            optimization: { inlineConst: true },
            output: {
              entryFileNames: "assets/[hash].js",
              chunkFileNames: "assets/[hash].js",
              codeSplitting: {
                groups: DESKTOP_NATIVE_CODE_SPLITTING_GROUPS,
              },
            },
          },
        }
      : {}),
  },
  test: {
    environment: "jsdom",
    setupFiles: ["./src/test/setup.ts"],
    restoreMocks: true,
    clearMocks: true,
    exclude: [
      ...configDefaults.exclude,
      "**/.claude/worktrees/**",
      "**/work/**",
    ],
  },
}));

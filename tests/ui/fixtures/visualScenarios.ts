import {
  acpProjectionStoreFor,
  clearAcpProjectionRegistry,
  pinAcpProjectionIds,
} from "../../../src/app/acpProjectionRegistry";
import { useAgentArtifactAdmissionStore } from "../../../src/app/agentArtifactAdmissionStore";
import { useWorkbenchStore } from "../../../src/app/store";
import { diffCacheKey } from "../../../src/domain/diffIdentity";
import { resourceKey } from "../../../src/domain/resourceIdentity";
import type {
  AcpCapabilitySet,
  AcpChatPage,
  AcpItem,
  AgentSessionProjection,
  EntityRevision,
  ProjectGeneration,
  ProjectionRevision,
  RuntimeEpoch,
  SageAgentSessionId,
  SageChatId,
  SageItemId,
  SageProjectId,
  SageTurnId,
  TurnProjection,
  TurnState,
} from "../../../src/domain/acp";
import type {
  EditorDocument,
  FileDiff,
  ResourceRef,
  WorkbenchTab,
} from "../../../src/domain/workbench";
import { workspaceSearchDesktop } from "../../../src/features/search/workspaceSearchDesktop";
import {
  applyBrowseIntent,
  clearBrowseSessions,
  setHarnessBrowseOwner,
} from "../../../src/features/preview/harnessBrowse";
import { acpDesktop } from "../../../src/services/acpDesktop";
import { languageDesktop } from "../../../src/services/languageDesktop";

export type VisualScenarioName =
  | "agent-single"
  | "agent-dual"
  | "agent-long"
  | "editor"
  | "diff"
  | "preview"
  | "error"
  | "waiting";

export type VisualScenarioMode = "agent" | "editor";
export type VisualScenarioResource = "empty" | "file" | "diff" | "preview";

export interface VisualScenarioDefinition {
  name: VisualScenarioName;
  mode: VisualScenarioMode;
  resource: VisualScenarioResource;
}

export const VISUAL_SCENARIOS: readonly VisualScenarioDefinition[] = [
  { name: "agent-single", mode: "agent", resource: "empty" },
  { name: "agent-dual", mode: "agent", resource: "empty" },
  { name: "agent-long", mode: "agent", resource: "empty" },
  { name: "editor", mode: "editor", resource: "file" },
  { name: "diff", mode: "agent", resource: "diff" },
  { name: "preview", mode: "agent", resource: "preview" },
  { name: "error", mode: "agent", resource: "empty" },
  { name: "waiting", mode: "agent", resource: "empty" },
];

const FIXED_NOW = Date.UTC(2026, 7, 31, 9, 30, 0);
const PROJECT_ID = "visual-project" as SageProjectId;
const ROOT_ID = "visual-root";
const GENERATION = "0" as ProjectGeneration;
const RUNTIME_EPOCH = "7" as RuntimeEpoch;
const SESSION_ID = "visual-session" as SageAgentSessionId;
const BINDING_ID = "visual-binding";
const MODEL_ID = "visual-model";

const capabilities: AcpCapabilitySet = {
  schemaVersion: 1,
  protocolVersion: "1.0",
  newSession: true,
  resumeSession: true,
  loadSession: true,
  closeSession: true,
  prompt: true,
  cancel: true,
  nativeSessionFork: false,
  activeTurnSteer: false,
  childConversationProjection: true,
  childTranscriptRead: true,
  collaborationEventProjection: true,
  multiRootExtensionSchemaId: null,
  modelCount: 1,
  configCount: 1,
};

function session(state: AgentSessionProjection["state"] = "ready") {
  return {
    id: SESSION_ID,
    ordinal: 1,
    bindingId: BINDING_ID,
    modelId: MODEL_ID,
    state,
    residency: "hot",
    runtimeEpoch: RUNTIME_EPOCH,
    nextRuntimeEpoch: null,
    capabilities,
    revision: "1",
    projectionRevision: "1",
    legacy: false,
  } as AgentSessionProjection;
}

interface ItemInput {
  chatId: SageChatId;
  turnId: SageTurnId;
  ordinal: number;
  createdAt: number;
}

function itemIdentity(input: ItemInput) {
  const id = `${input.turnId}-item-${input.ordinal}` as SageItemId;
  return {
    id,
    projectId: PROJECT_ID,
    chatId: input.chatId,
    sessionId: SESSION_ID,
    turnId: input.turnId,
    runtimeEpoch: RUNTIME_EPOCH,
    ordinal: input.ordinal,
    origin: "primary" as const,
    activityId: null,
    lifecycle: "completed" as const,
    revision: "1" as EntityRevision,
    createdAt: input.createdAt,
    updatedAt: input.createdAt + 1_000,
    terminalAt: input.createdAt + 1_000,
  };
}

function textItem(
  input: ItemInput,
  role: "user" | "assistant" | "system",
  text: string,
): AcpItem {
  return {
    ...itemIdentity(input),
    kind: "text",
    toolState: null,
    payload: { role, text },
  };
}

function thoughtItem(input: ItemInput, summary: string): AcpItem {
  return {
    ...itemIdentity(input),
    kind: "thoughtSummary",
    toolState: null,
    payload: { summary },
  };
}

function planItem(input: ItemInput): AcpItem {
  return {
    ...itemIdentity(input),
    kind: "plan",
    toolState: null,
    payload: {
      entries: [
        { label: "Inspect the current hierarchy", state: "completed" },
        { label: "Tighten the interaction states", state: "completed" },
        { label: "Verify narrow layouts", state: "inProgress" },
      ],
    },
  };
}

function toolItem(
  input: ItemInput,
  state: "completed" | "inProgress",
): AcpItem {
  const identity = itemIdentity(input);
  return {
    ...identity,
    lifecycle: state === "completed" ? "completed" : "streaming",
    terminalAt: state === "completed" ? identity.terminalAt : null,
    kind: "toolCall",
    toolState: state,
    payload: {
      presentation: {
        kind: "search",
        label: "Search workspace",
        querySummary: "interaction states",
        resources: [],
      },
      inputSummary: "Scanning relevant UI modules",
    },
  };
}

function errorItem(input: ItemInput): AcpItem {
  return {
    ...itemIdentity(input),
    kind: "error",
    toolState: null,
    lifecycle: "failed",
    payload: {
      code: "PREVIEW_UNAVAILABLE",
      message:
        "The preview process stopped unexpectedly. The Chat is preserved and can be retried.",
      recoverable: true,
    },
  };
}

function fileChangeItem(input: ItemInput): AcpItem {
  return {
    ...itemIdentity(input),
    kind: "fileChangeDiff",
    toolState: null,
    payload: {
      changes: [
        {
          resource: {
            kind: "resource",
            resource: {
              rootId: ROOT_ID,
              relativePath: "src/workbench/AgentChatPanes.tsx",
            },
          },
          operation: "updated",
          originalResource: null,
          summary: "Refined the dual-chat layout",
          additions: 28,
          deletions: 11,
        },
        {
          resource: {
            kind: "resource",
            resource: {
              rootId: ROOT_ID,
              relativePath: "src/styles/workbench.css",
            },
          },
          operation: "updated",
          originalResource: null,
          summary: "Aligned spacing and focus states",
          additions: 16,
          deletions: 7,
        },
      ],
      truncated: false,
    },
  };
}

function turn(
  chatId: SageChatId,
  ordinal: number,
  state: TurnState,
  items: AcpItem[],
): TurnProjection {
  const startedAt = FIXED_NOW - (20 - ordinal) * 60_000;
  const terminal = [
    "completed",
    "interrupted",
    "cancelled",
    "failed",
    "deliveryUnknown",
  ].includes(state);
  return {
    id: `visual-turn-${chatId}-${ordinal}` as SageTurnId,
    sessionId: SESSION_ID,
    runtimeEpoch: RUNTIME_EPOCH,
    ordinal,
    state,
    revision: "1" as EntityRevision,
    createdAt: startedAt,
    updatedAt: startedAt + 20_000,
    terminalAt: terminal ? startedAt + 20_000 : null,
    items,
    activities: [],
  };
}

function turnItems(
  chatId: SageChatId,
  ordinal: number,
  options: { rich?: boolean; working?: boolean; failed?: boolean } = {},
) {
  const turnId = `visual-turn-${chatId}-${ordinal}` as SageTurnId;
  const createdAt = FIXED_NOW - (20 - ordinal) * 60_000;
  const input = (itemOrdinal: number): ItemInput => ({
    chatId,
    turnId,
    ordinal: itemOrdinal,
    createdAt: createdAt + itemOrdinal * 1_000,
  });
  const items: AcpItem[] = [
    textItem(
      input(1),
      "user",
      ordinal === 1
        ? "Review the Agent and Editor surfaces and make the experience feel coherent."
        : `Check refinement pass ${ordinal} and keep the existing mode architecture intact.`,
    ),
  ];
  if (options.rich || options.working) {
    items.push(
      thoughtItem(
        input(2),
        "I’m comparing the hierarchy, density, and state language before changing the presentation.",
      ),
    );
  }
  if (options.rich) {
    items.push(planItem(input(3)), toolItem(input(4), "completed"));
  }
  if (options.working) {
    items.push(toolItem(input(3), "inProgress"));
  } else if (options.failed) {
    items.push(errorItem(input(3)));
  } else {
    items.push(
      textItem(
        input(options.rich ? 5 : 2),
        "assistant",
        options.rich
          ? "The core hierarchy is now consistent.\n\n```ts\nconst panes = Math.min(openChats.length, 2);\n```\n\nThe next pass verifies focus, narrow width, and reduced motion."
          : "This pass is stable. Spacing, actions, and state labels remain readable without moving the underlying product seams.",
      ),
    );
  }
  if (options.rich) items.push(fileChangeItem(input(6)));
  return items;
}

function page(
  chatIdValue: string,
  title: string,
  turns: TurnProjection[],
  sessionState: AgentSessionProjection["state"] = "ready",
): AcpChatPage {
  const chatId = chatIdValue as SageChatId;
  return {
    projectId: PROJECT_ID,
    chatId,
    title,
    projectionRevision: "42" as ProjectionRevision,
    currentSessionId: SESSION_ID,
    sessions: [session(sessionState)],
    turns,
    nextCursor: null,
  };
}

function standardPage(
  chatIdValue: string,
  title: string,
  variant: "rich" | "simple" | "error" | "waiting",
) {
  const chatId = chatIdValue as SageChatId;
  if (variant === "error") {
    return page(chatId, title, [
      turn(chatId, 1, "failed", turnItems(chatId, 1, { failed: true })),
    ]);
  }
  if (variant === "waiting") {
    return page(
      chatId,
      title,
      [turn(chatId, 1, "waitingTool", turnItems(chatId, 1, { working: true }))],
      "active",
    );
  }
  return page(chatId, title, [
    turn(
      chatId,
      1,
      "completed",
      turnItems(chatId, 1, { rich: variant === "rich" }),
    ),
    turn(chatId, 2, "completed", turnItems(chatId, 2)),
  ]);
}

function longPage() {
  const chatId = "visual-chat-long" as SageChatId;
  const turns = Array.from({ length: 14 }, (_, index) => {
    const ordinal = index + 1;
    const working = ordinal === 14;
    return turn(
      chatId,
      ordinal,
      working ? "running" : "completed",
      turnItems(chatId, ordinal, { rich: ordinal === 1, working }),
    );
  });
  return page(chatId, "Long transcript review", turns, "active");
}

export interface BuiltVisualScenario extends VisualScenarioDefinition {
  pages: AcpChatPage[];
}

export function buildVisualScenario(
  name: VisualScenarioName,
): BuiltVisualScenario {
  const definition = VISUAL_SCENARIOS.find((entry) => entry.name === name);
  if (!definition) throw new Error(`Unknown visual scenario: ${name}`);
  const pages =
    name === "agent-dual"
      ? [
          standardPage(
            "visual-chat-primary",
            "Agent navigation polish",
            "rich",
          ),
          standardPage(
            "visual-chat-secondary",
            "Editor consistency pass",
            "simple",
          ),
        ]
      : name === "agent-long"
        ? [longPage()]
        : name === "error"
          ? [
              standardPage(
                "visual-chat-error",
                "Recover a failed preview",
                "error",
              ),
            ]
          : name === "waiting"
            ? [
                standardPage(
                  "visual-chat-waiting",
                  "Inspecting workspace",
                  "waiting",
                ),
              ]
            : [
                standardPage(
                  `visual-chat-${name}`,
                  name === "preview"
                    ? "Preview interaction"
                    : "Agent navigation polish",
                  "rich",
                ),
              ];
  return { ...definition, pages };
}

const fileResource: ResourceRef = {
  rootId: ROOT_ID,
  relativePath: "src/workbench/AgentChatPanes.tsx",
};

function installEditorToolFixtures() {
  languageDesktop.startLanguageSession = () => new Promise<number>(() => {});
  languageDesktop.cancelLanguageSession = async () => undefined;
  languageDesktop.subscribeDiagnostics = async (onReport) => {
    requestAnimationFrame(() =>
      requestAnimationFrame(() =>
        onReport({
          resource: fileResource,
          generation: 1,
          diagnostics: [
            {
              resource: fileResource,
              message: "maximum should be named maxVisibleChats",
              severity: "warning",
              startLine: 4,
              startColumn: 43,
              endLine: 4,
              endColumn: 50,
            },
          ],
        }),
      ),
    );
    return () => undefined;
  };
  languageDesktop.subscribeOutline = async (onReport) => {
    requestAnimationFrame(() =>
      requestAnimationFrame(() =>
        onReport({
          resource: fileResource,
          generation: 1,
          symbols: [
            {
              resource: fileResource,
              name: "AgentChatPanes",
              kind: "function",
              startLine: 3,
            },
          ],
        }),
      ),
    );
    return () => undefined;
  };
  workspaceSearchDesktop.search = async (request) => ({
    requestId: "visual-search",
    projectId: request.projectId,
    transitionEpoch: request.expectedTransitionEpoch,
    indexGeneration: request.indexGeneration,
    query: request.query,
    matches: [
      {
        resource: fileResource,
        lineNumber: 3,
        columnNumber: 17,
        preview: "export function AgentChatPanes() {",
      },
    ],
    roots: [
      {
        rootId: ROOT_ID,
        rootName: "Sage",
        state: "complete",
        filesScanned: 124,
        bytesScanned: 84_200,
        entriesVisited: 139,
        skippedFiles: 0,
        errorCount: 0,
        message: null,
      },
    ],
    truncated: false,
    continuationToken: null,
    limitReasons: [],
    elapsedMs: 8,
  });
}

const editorContent = [
  'import { ChatSurface } from "../features/chat/ChatSurface";',
  "",
  "export function AgentChatPanes() {",
  "  const visibleChats = useVisibleChats({ maximum: 2 });",
  "",
  "  return visibleChats.map((chat) => (",
  "    <ChatSurface key={chat.id} chatId={chat.id} />",
  "  ));",
  "}",
].join("\n");

function editorDocument(): EditorDocument {
  const version = {
    fingerprint: "visual-editor-v1",
    byteLen: editorContent.length,
    modifiedAt: FIXED_NOW - 90_000,
  };
  return {
    resource: fileResource,
    disk: version,
    base: version,
    buffer: {
      ...version,
      revision: 0,
      content: editorContent,
    },
    lineEnding: "lf",
    dirty: false,
    conflict: null,
  };
}

const visualDiff: FileDiff = {
  resource: fileResource,
  binary: false,
  truncated: false,
  hunks: [
    {
      header: "@@ -18,7 +18,10 @@ export function AgentChatPanes()",
      lines: [
        {
          kind: "context",
          oldLine: 18,
          newLine: 18,
          text: "const chats = visibleChats();",
        },
        {
          kind: "removed",
          oldLine: 19,
          newLine: null,
          text: "return <ChatSurface chatId={chats[0].id} />;",
        },
        {
          kind: "added",
          oldLine: null,
          newLine: 19,
          text: "return chats.slice(0, 2).map((chat) => (",
        },
        {
          kind: "added",
          oldLine: null,
          newLine: 20,
          text: "  <ChatSurface key={chat.id} chatId={chat.id} />",
        },
        { kind: "added", oldLine: null, newLine: 21, text: "));" },
      ],
    },
  ],
};

function chatTab(pageValue: AcpChatPage, position: number): WorkbenchTab {
  const nonterminal = pageValue.turns.some((candidate) =>
    ["running", "waitingTool", "waitingApproval", "waitingModel"].includes(
      candidate.state,
    ),
  );
  return {
    id: pageValue.chatId,
    kind: "chat",
    title: pageValue.title,
    position,
    pinned: position === 0,
    dirty: false,
    status: nonterminal ? "working" : "complete",
    updatedAt: FIXED_NOW - position * 60_000,
  };
}

function fixtureTabs(scenario: BuiltVisualScenario): WorkbenchTab[] {
  const chats = scenario.pages.map(chatTab);
  if (scenario.resource === "file") {
    return [
      ...chats,
      {
        id: "visual-file-tab",
        kind: "file",
        title: "AgentChatPanes.tsx",
        position: chats.length,
        pinned: false,
        dirty: false,
        resource: fileResource,
      },
    ];
  }
  if (scenario.resource === "diff") {
    return [
      ...chats,
      {
        id: "visual-diff-tab",
        kind: "diff",
        title: "AgentChatPanes.tsx",
        position: chats.length,
        pinned: false,
        dirty: false,
        resource: fileResource,
      },
    ];
  }
  return chats;
}

export async function installVisualScenario(name: VisualScenarioName) {
  const scenario = buildVisualScenario(name);
  const tabs = fixtureTabs(scenario);
  const chatIds = scenario.pages.map((pageValue) => pageValue.chatId);
  const activeTabId =
    scenario.resource === "file"
      ? "visual-file-tab"
      : scenario.resource === "diff"
        ? "visual-diff-tab"
        : (chatIds[0] ?? null);

  clearAcpProjectionRegistry();
  clearBrowseSessions();
  useAgentArtifactAdmissionStore.getState().clear();
  pinAcpProjectionIds(chatIds);
  installEditorToolFixtures();

  const pagesByChat = new Map(
    scenario.pages.map((pageValue) => [pageValue.chatId, pageValue]),
  );
  acpDesktop.getChatPage = async (request) => {
    const pageValue = pagesByChat.get(request.chatId);
    if (!pageValue) throw new Error(`No visual page for ${request.chatId}`);
    return structuredClone(pageValue);
  };

  for (const pageValue of scenario.pages) {
    const scope = {
      projectId: PROJECT_ID,
      projectGeneration: GENERATION,
      chatId: pageValue.chatId,
    };
    const store = acpProjectionStoreFor(pageValue.chatId);
    store.getState().activateScope(scope);
    store.getState().applyAuthoritativePage(scope, structuredClone(pageValue));
  }

  const editorDoc = editorDocument();
  const emptyOwnership = {
    composerText: "",
    sending: false,
    error: null,
    requestGeneration: 0,
  };
  const chatUiById = Object.fromEntries(
    chatIds.map((chatId) => [chatId, { ...emptyOwnership }]),
  );
  const rootListing = {
    resource: { rootId: ROOT_ID, relativePath: "" },
    entries: [
      {
        name: "src",
        resource: { rootId: ROOT_ID, relativePath: "src" },
        kind: "directory" as const,
        byteLen: null,
      },
      {
        name: "README.md",
        resource: { rootId: ROOT_ID, relativePath: "README.md" },
        kind: "file" as const,
        byteLen: 2_048,
      },
    ],
    truncated: false,
  };

  useWorkbenchStore.setState({
    phase: "ready",
    error: null,
    projects: [
      {
        id: PROJECT_ID,
        name: "Sage",
        rootCount: 1,
        lastOpenedAt: FIXED_NOW,
        railChats: [
          ...scenario.pages.map((pageValue, index) => ({
            id: pageValue.chatId,
            title: pageValue.title,
            pinned: index === 0,
            position: index,
            status:
              name === "error"
                ? ("failed" as const)
                : name === "waiting"
                  ? ("waiting" as const)
                  : index === 1
                    ? ("working" as const)
                    : ("complete" as const),
            updatedAt: FIXED_NOW - index * 60_000,
          })),
          {
            id: "visual-chat-attention",
            title: "Keyboard walkthrough needs attention",
            pinned: false,
            position: 8,
            status: "waiting",
            updatedAt: FIXED_NOW - 5 * 60_000,
          },
        ],
        railChatsTruncated: false,
      },
    ],
    project: {
      id: PROJECT_ID,
      name: "Sage",
      roots: [
        {
          id: ROOT_ID,
          name: "Sage",
          ordinal: 0,
          availability: "available",
          message: null,
        },
      ],
    },
    projectGeneration: 0,
    nativeTransitionEpoch: 0,
    projectTransitioning: false,
    tabs,
    archivedChats: [],
    activeTabId,
    selectedRootId: ROOT_ID,
    selectedResource: scenario.resource === "file" ? fileResource : null,
    sidebarOpen: true,
    sidebarView: "explorer",
    harnesses: [
      {
        id: "visual-acp",
        name: "Codex",
        description: "Deterministic ACP visual fixture",
        installState: "available",
        source: "development",
        accountModelDiscovery: false,
        transport: "acp",
        bindingId: BINDING_ID,
      },
    ],
    selectedHarnessId: "visual-acp",
    models: [{ id: MODEL_ID, name: "GPT-5" }],
    selectedModelId: MODEL_ID,
    harnessDiscoveryPhase: "ready",
    harnessDiscoveryError: null,
    sessionId: SESSION_ID,
    messages: [],
    chatUiById,
    chatError: null,
    composerText: "",
    sending: false,
    repositories: {
      roots: [
        {
          rootId: ROOT_ID,
          state: "repository",
          branch: "feat/wave2-ui",
          changes: [
            {
              resource: fileResource,
              indexStatus: "unchanged",
              worktreeStatus: "modified",
              additions: 28,
              deletions: 11,
            },
          ],
          truncated: false,
          elapsedMs: 12,
          message: null,
        },
      ],
      truncated: false,
      elapsedMs: 12,
    },
    turnChanges: [],
    directoryListings: {
      [resourceKey(rootListing.resource)]: {
        phase: "ready",
        value: rootListing,
      },
    },
    expandedDirectories: {},
    diffs: {
      [diffCacheKey(undefined, fileResource)]: {
        phase: "ready",
        value: visualDiff,
      },
    },
    activeEditor:
      scenario.resource === "file"
        ? {
            phase: "ready",
            resource: fileResource,
            document: editorDoc,
            content: editorContent,
            revision: 0,
            persistedRevision: 0,
            persisting: false,
            saving: false,
            message: null,
          }
        : null,
    loadRepositoryInventory: async () => undefined,
    loadDirectory: async () => undefined,
    loadDiff: async () => undefined,
  });

  if (scenario.resource === "diff") {
    useAgentArtifactAdmissionStore.getState().admit("visual-diff-tab");
  }
  setHarnessBrowseOwner(chatIds[0] ?? null);
  if (scenario.resource === "preview" && chatIds[0]) {
    applyBrowseIntent({
      chatId: chatIds[0],
      sessionId: SESSION_ID,
      url: `${window.location.origin}/tests/ui/fixtures/preview.html`,
    });
  }
  document.documentElement.dataset.visualScenario = name;
  await new Promise((resolve) =>
    requestAnimationFrame(() => resolve(undefined)),
  );
}

export function installVisualRepositoryError() {
  useWorkbenchStore.setState({
    repositoryInventoryError: "Could not refresh repository inventory.",
    repositoryInventoryPhase: "ready",
    repositoryStatusPhase: "ready",
  });
}

export function installVisualRepositoryLoading() {
  useWorkbenchStore.setState({
    repositoryInventoryError: null,
    repositoryStatusError: null,
    repositoryInventoryPhase: "loading",
    repositoryStatusPhase: "loading",
  });
}

export function installVisualEmptyEditorTools() {
  languageDesktop.subscribeDiagnostics = async () => () => undefined;
  languageDesktop.subscribeOutline = async () => () => undefined;
}

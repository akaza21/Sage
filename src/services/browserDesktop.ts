import type {
  BootstrapSnapshot,
  ChatMessage,
  DirectoryListing,
  FileDiff,
  FileChange,
  ProjectLayoutRequest,
  ProjectRepositoryStatus,
  ProjectSwitchRequest,
  ProjectTransitionResult,
  ResourceRef,
} from "../domain/workbench";
import { resourceKey } from "../domain/resourceIdentity";
import { browserEditor } from "./browserEditor";

const rootId = "browser-root";
const ref = (relativePath: string): ResourceRef => ({ rootId, relativePath });
const changes: FileChange[] = [
  {
    resource: ref("src/workbench/Workbench.tsx"),
    indexStatus: "unchanged",
    worktreeStatus: "modified",
    additions: 38,
    deletions: 14,
  },
];
const repository: ProjectRepositoryStatus = {
  roots: [
    {
      rootId,
      state: "repository",
      branch: "main",
      changes,
      truncated: false,
      elapsedMs: 18,
      message: null,
    },
  ],
  truncated: false,
  elapsedMs: 18,
};
const snapshot: BootstrapSnapshot = {
  transitionEpoch: 0,
  projects: [
    { id: "browser-project", name: "Sage", rootCount: 1, lastOpenedAt: 1 },
  ],
  project: {
    id: "browser-project",
    name: "Sage",
    roots: [
      {
        id: rootId,
        name: "Sage",
        ordinal: 0,
        availability: "available",
        message: null,
      },
    ],
  },
  tabs: [
    {
      id: "tab-chat",
      kind: "chat",
      title: "Chat: Walking skeleton",
      position: 0,
      pinned: true,
      dirty: false,
      status: "idle",
    },
  ],
  activeTabId: "tab-chat",
  selectedRootId: rootId,
  selectedResource: null,
  harnesses: [
    {
      id: "claude-code",
      name: "Claude Code",
      description: "Curated ACP adapter, installed only when requested.",
      installState: "notInstalled",
      source: "builtIn",
    },
    {
      id: "codex",
      name: "Codex",
      description: "Curated ACP adapter, installed only when requested.",
      installState: "notInstalled",
      source: "builtIn",
    },
    {
      id: "pi",
      name: "Pi",
      description: "Curated ACP adapter, installed only when requested.",
      installState: "notInstalled",
      source: "builtIn",
    },
    {
      id: "sage-simulator",
      name: "Sage ACP Simulator",
      description: "Deterministic development harness.",
      installState: "available",
      source: "development",
    },
  ],
  sessionId: "session-walking-skeleton",
  selectedHarnessId: "sage-simulator",
  selectedModelId: "sim-fast",
  messages: [
    {
      id: "message-welcome",
      sessionId: "session-walking-skeleton",
      sequence: 1,
      role: "assistant",
      text: "The Sage walking skeleton is ready.",
      createdAt: Date.now(),
    },
  ],
  repositories: repository,
  turnChanges: changes,
  recoveredBuffers: [],
  recoveryNotice: null,
};

const directories = new Map<string, DirectoryListing>([
  [
    resourceKey(ref("")),
    {
      resource: ref(""),
      truncated: false,
      entries: [
        {
          name: "src",
          resource: ref("src"),
          kind: "directory",
          byteLen: null,
        },
        {
          name: "README.md",
          resource: ref("README.md"),
          kind: "file",
          byteLen: 1_840,
        },
      ],
    },
  ],
  [
    resourceKey(ref("src")),
    {
      resource: ref("src"),
      truncated: false,
      entries: [
        {
          name: "workbench",
          resource: ref("src/workbench"),
          kind: "directory",
          byteLen: null,
        },
      ],
    },
  ],
  [
    resourceKey(ref("src/workbench")),
    {
      resource: ref("src/workbench"),
      truncated: false,
      entries: [
        {
          name: "Workbench.tsx",
          resource: ref("src/workbench/Workbench.tsx"),
          kind: "file",
          byteLen: 2_180,
        },
      ],
    },
  ],
]);

let messages = [...snapshot.messages];
const MAX_BROWSER_MESSAGES = 500;
const MAX_BROWSER_MESSAGE_BYTES = 4 * 1024 * 1024;

function browserMessageBytes(values: ChatMessage[]) {
  const encoder = new TextEncoder();
  return values.reduce(
    (total, message) => total + encoder.encode(message.text).byteLength,
    0,
  );
}

export function appendBrowserSimulatorTurn(
  current: ChatMessage[],
  request: {
    sessionId: string;
    modelId: string | null;
    text: string;
  },
  ids: [string, string] = [crypto.randomUUID(), crypto.randomUUID()],
  createdAt = Date.now(),
) {
  const sequence = (current.at(-1)?.sequence ?? 0) + 1;
  const response = `Simulator acknowledged the prompt using ${
    request.modelId ?? "the harness default"
  }. The ACP supervisor remains a later phase.`;
  const turn: ChatMessage[] = [
    {
      id: ids[0],
      sessionId: request.sessionId,
      sequence,
      role: "user",
      text: request.text,
      createdAt,
    },
    {
      id: ids[1],
      sessionId: request.sessionId,
      sequence: sequence + 1,
      role: "assistant",
      text: response,
      createdAt,
    },
  ];
  if (
    current.length + turn.length > MAX_BROWSER_MESSAGES ||
    browserMessageBytes(current) + browserMessageBytes(turn) >
      MAX_BROWSER_MESSAGE_BYTES
  ) {
    throw new Error(
      "browser session history capacity reached; existing messages were preserved",
    );
  }
  return [...current, ...turn];
}

export const browserDesktop = {
  async getBootstrapSnapshot() {
    return structuredClone({ ...snapshot, messages });
  },
  async getRepositoryStatus() {
    return structuredClone(repository);
  },
  async listDirectory(resource: ResourceRef) {
    return structuredClone(
      directories.get(resourceKey(resource)) ?? {
        resource,
        entries: [],
        truncated: false,
      },
    );
  },
  readTextFile: browserEditor.readTextFile,
  openEditorDocument: browserEditor.open,
  snapshotEditorBuffer: browserEditor.snapshot,
  saveEditorDocument: browserEditor.save,
  discardEditorBuffer: browserEditor.discard,
  async getFileDiff(resource: ResourceRef): Promise<FileDiff> {
    return { resource, binary: false, truncated: false, hunks: [] };
  },
  async persistProjectLayout(request: ProjectLayoutRequest) {
    snapshot.tabs = structuredClone(request.tabs);
    snapshot.activeTabId = request.activeTabId;
    snapshot.selectedRootId = request.selectedRootId;
    snapshot.selectedResource = structuredClone(request.selectedResource);
  },
  async switchProject(
    request: ProjectSwitchRequest,
    expectedTransitionEpoch: number,
  ): Promise<ProjectTransitionResult> {
    if (expectedTransitionEpoch !== snapshot.transitionEpoch) {
      throw new Error("project changed before the requested transition");
    }
    await this.persistProjectLayout(request.currentLayout);
    if (request.targetProjectId !== snapshot.project?.id) {
      throw new Error("Browser fixture has one project.");
    }
    snapshot.transitionEpoch += 1;
    return {
      committed: true,
      transitionEpoch: snapshot.transitionEpoch,
      snapshot: structuredClone({ ...snapshot, messages }),
      recoveryError: null,
    };
  },
  async listHarnessModels(harnessId: string) {
    return harnessId === "sage-simulator"
      ? [{ id: "sim-fast", name: "Simulator Fast" }]
      : [];
  },
  async sendPrompt(request: {
    sessionId: string;
    harnessId: string;
    modelId: string | null;
    text: string;
  }): Promise<ChatMessage[]> {
    messages = appendBrowserSimulatorTurn(messages, request);
    return structuredClone(messages);
  },
};

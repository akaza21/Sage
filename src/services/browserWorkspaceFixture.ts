import { resourceKey } from "../domain/resourceIdentity";
import type {
  BootstrapSnapshot,
  DirectoryListing,
  FileChange,
  ProjectRepositoryStatus,
  ResourceRef,
} from "../domain/workbench";

export const browserRootId = "browser-root";
export const browserRef = (relativePath: string): ResourceRef => ({
  rootId: browserRootId,
  relativePath,
});

export const browserChanges: FileChange[] = [
  {
    resource: browserRef("src/workbench/Workbench.tsx"),
    indexStatus: "unchanged",
    worktreeStatus: "modified",
    additions: 38,
    deletions: 14,
  },
];

export const browserRepository: ProjectRepositoryStatus = {
  roots: [
    {
      rootId: browserRootId,
      state: "repository",
      branch: "main",
      changes: browserChanges,
      truncated: false,
      elapsedMs: 18,
      message: null,
    },
  ],
  truncated: false,
  elapsedMs: 18,
};

export const browserSnapshot: BootstrapSnapshot = {
  transitionEpoch: 0,
  projects: [
    { id: "browser-project", name: "Sage", rootCount: 1, lastOpenedAt: 1 },
  ],
  project: {
    id: "browser-project",
    name: "Sage",
    roots: [
      {
        id: browserRootId,
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
  archivedChats: [],
  activeTabId: "tab-chat",
  selectedRootId: browserRootId,
  selectedResource: null,
  harnesses: [
    {
      id: "codex",
      name: "Codex",
      description: "Curated ACP adapter, installed only when requested.",
      installState: "notInstalled",
      source: "builtIn",
      transport: "acp",
      bindingId: "codex-app-server",
    },
    {
      id: "pi",
      name: "Pi",
      description: "Curated ACP adapter, installed only when requested.",
      installState: "notInstalled",
      source: "builtIn",
      transport: "acp",
      bindingId: null,
    },
    {
      id: "claude-code",
      name: "Claude Code",
      description: "Curated ACP adapter, installed only when requested.",
      installState: "notInstalled",
      source: "builtIn",
      transport: "acp",
      bindingId: null,
    },
    {
      id: "oh-my-pi",
      name: "Oh My Pi",
      description: "Curated native ACP binding, admitted last.",
      installState: "notInstalled",
      source: "builtIn",
      transport: "acp",
      bindingId: null,
    },
    {
      id: "sage-simulator",
      name: "Sage ACP Simulator",
      description: "Deterministic development harness.",
      installState: "available",
      source: "development",
      transport: "legacySimulator",
      bindingId: null,
    },
  ],
  sessionId: null,
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
  repositories: browserRepository,
  turnChanges: browserChanges,
  recoveredBuffers: [],
  recoveryNotice: null,
};

export const browserDirectories = new Map<string, DirectoryListing>([
  [
    resourceKey(browserRef("")),
    {
      resource: browserRef(""),
      truncated: false,
      entries: [
        {
          name: "src",
          resource: browserRef("src"),
          kind: "directory",
          byteLen: null,
        },
        {
          name: "README.md",
          resource: browserRef("README.md"),
          kind: "file",
          byteLen: 1_840,
        },
      ],
    },
  ],
  [
    resourceKey(browserRef("src")),
    {
      resource: browserRef("src"),
      truncated: false,
      entries: [
        {
          name: "workbench",
          resource: browserRef("src/workbench"),
          kind: "directory",
          byteLen: null,
        },
      ],
    },
  ],
  [
    resourceKey(browserRef("src/workbench")),
    {
      resource: browserRef("src/workbench"),
      truncated: false,
      entries: [
        {
          name: "Workbench.tsx",
          resource: browserRef("src/workbench/Workbench.tsx"),
          kind: "file",
          byteLen: 2_180,
        },
      ],
    },
  ],
]);

const initialBrowserDirectories = new Map(
  [...browserDirectories].map(([key, listing]) => [
    key,
    structuredClone(listing),
  ]),
);

export function resetBrowserDirectories() {
  browserDirectories.clear();
  for (const [key, listing] of initialBrowserDirectories) {
    browserDirectories.set(key, structuredClone(listing));
  }
}

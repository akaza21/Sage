import type {
  ArchivedChatSummary,
  ChatMessage,
  ProjectLayoutRequest,
  WorkbenchTab,
} from "./chat";
import type { RecoveredEditorBuffer } from "./editor";
import type { HarnessDefinition } from "./harness";
import type { FileChange, ProjectRepositoryStatus } from "./repository";
import type { ResourceRef } from "./resource";

export interface ProjectListItem {
  id: string;
  name: string;
  rootCount: number;
  lastOpenedAt: number;
}

export type RootAvailability =
  "available" | "missing" | "permissionDenied" | "invalid" | "unavailable";

export interface ProjectRootSummary {
  id: string;
  name: string;
  ordinal: number;
  availability: RootAvailability;
  message: string | null;
}

export interface ProjectSummary {
  id: string;
  name: string;
  roots: ProjectRootSummary[];
}

export type WorkspaceEntryKind = "directory" | "file" | "symlink" | "other";

export interface WorkspaceEntry {
  name: string;
  resource: ResourceRef;
  kind: WorkspaceEntryKind;
  byteLen: number | null;
}

export interface DirectoryListing {
  resource: ResourceRef;
  entries: WorkspaceEntry[];
  truncated: boolean;
}

export interface CreateProjectFileRequest {
  projectId: string;
  projectGeneration: number;
  expectedTransitionEpoch: number;
  parent: ResourceRef;
  basename: string;
}

export interface CreateProjectFileResult {
  outcome: "committed" | "effectUnknown";
  resourceConfirmed: boolean;
  projectId: string;
  projectGeneration: number;
  transitionEpoch: number;
  resource: ResourceRef;
  parentListing: DirectoryListing;
  warning: string | null;
}

export type CreateProjectFolderRequest = CreateProjectFileRequest;
export type CreateProjectFolderResult = CreateProjectFileResult;

export interface RenameProjectFileRequest {
  projectId: string;
  projectGeneration: number;
  expectedTransitionEpoch: number;
  source: ResourceRef;
  basename: string;
  currentLayout: ProjectLayoutRequest;
}

export interface RenameProjectFileResult {
  outcome: "committed" | "effectUnknown";
  resourceConfirmed: boolean;
  projectId: string;
  projectGeneration: number;
  transitionEpoch: number;
  source: ResourceRef;
  target: ResourceRef;
  parentListing: DirectoryListing | null;
  snapshot: BootstrapSnapshot | null;
  warning: string | null;
}

export type RenameProjectFolderRequest = RenameProjectFileRequest;
export type RenameProjectFolderResult = RenameProjectFileResult;

export type TrashResourceKind = "file" | "emptyFolder";

export interface TrashProjectResourceRequest {
  projectId: string;
  projectGeneration: number;
  expectedTransitionEpoch: number;
  source: ResourceRef;
  kind: TrashResourceKind;
  currentLayout: ProjectLayoutRequest;
}

export interface TrashProjectResourceResult {
  outcome: "committed" | "effectUnknown" | "unsupported";
  resourceConfirmed: boolean;
  projectId: string;
  projectGeneration: number;
  transitionEpoch: number;
  source: ResourceRef;
  parent: ResourceRef;
  parentListing: DirectoryListing | null;
  snapshot: BootstrapSnapshot | null;
  warning: string | null;
}

export interface ProjectSwitchRequest {
  targetProjectId: string;
  currentLayout: ProjectLayoutRequest;
}

export interface ProjectTransitionResult {
  committed: boolean;
  transitionEpoch: number;
  snapshot: BootstrapSnapshot | null;
  recoveryError: string | null;
}

export interface BootstrapSnapshot {
  transitionEpoch: number;
  projects: ProjectListItem[];
  project: ProjectSummary | null;
  tabs: WorkbenchTab[];
  archivedChats: ArchivedChatSummary[];
  activeTabId: string | null;
  selectedRootId: string | null;
  selectedResource: ResourceRef | null;
  harnesses: HarnessDefinition[];
  sessionId: string | null;
  selectedHarnessId: string | null;
  selectedModelId: string | null;
  messages: ChatMessage[];
  repositories: ProjectRepositoryStatus;
  turnChanges: FileChange[];
  recoveredBuffers: RecoveredEditorBuffer[];
  recoveryNotice: string | null;
}

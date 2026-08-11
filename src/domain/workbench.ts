export type TabKind =
  | "chat"
  | "file"
  | "diff"
  | "terminal"
  | "search"
  | "problems"
  | "output"
  | "settings";

export type SessionStatus =
  "idle" | "working" | "waiting" | "recovering" | "failed" | "complete";

export interface ResourceRef {
  rootId: string;
  relativePath: string;
}

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

export interface WorkbenchTab {
  id: string;
  kind: TabKind;
  title: string;
  position: number;
  pinned: boolean;
  dirty: boolean;
  status?: SessionStatus;
  resource?: ResourceRef;
}

export type GitFileState =
  | "unchanged"
  | "modified"
  | "typeChanged"
  | "added"
  | "deleted"
  | "renamed"
  | "copied"
  | "conflicted"
  | "untracked"
  | "unknown";

export interface FileChange {
  resource: ResourceRef;
  originalResource?: ResourceRef;
  indexStatus: GitFileState;
  worktreeStatus: GitFileState;
  additions: number | null;
  deletions: number | null;
}

export type RepositoryRootState =
  "repository" | "notRepository" | "unavailable";

export interface RootRepositoryStatus {
  rootId: string;
  state: RepositoryRootState;
  branch: string | null;
  changes: FileChange[];
  truncated: boolean;
  elapsedMs: number;
  message: string | null;
}

export interface ProjectRepositoryStatus {
  roots: RootRepositoryStatus[];
  truncated: boolean;
  elapsedMs: number;
}

export type GitFileMutationAction = "stage" | "unstage";

export interface GitFileMutationRequest {
  projectId: string;
  resource: ResourceRef;
  action: GitFileMutationAction;
}

export type GitFileMutationErrorKind = "effectUnknown";

export interface GitFileMutationError {
  kind: GitFileMutationErrorKind;
  message: string;
}

export interface GitFileMutationResult {
  projectId: string;
  transitionEpoch: number;
  repositories: ProjectRepositoryStatus;
  error: GitFileMutationError | null;
}

export type GitProcessState = "terminatedUnreaped" | "potentiallyRunning";

export type GitFileMutationCommandError =
  | { kind: "failed"; message: string }
  | {
      kind: "containment";
      processState: GitProcessState;
      message: string;
    };

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

export interface TextFile {
  resource: ResourceRef;
  content: string;
  byteLen: number;
  lineEnding: "lf" | "crlf" | "none";
  modifiedAt: number | null;
}

export interface DocumentVersion {
  fingerprint: string;
  byteLen: number;
  modifiedAt: number | null;
}

export interface EditorBufferVersion {
  fingerprint: string;
  byteLen: number;
  revision: number;
  content: string;
}

export interface EditorConflict {
  kind: "external-change";
  message: string;
}

export interface EditorDocument {
  resource: ResourceRef;
  disk: DocumentVersion;
  base: DocumentVersion;
  buffer: EditorBufferVersion;
  lineEnding: "lf" | "crlf" | "none";
  dirty: boolean;
  conflict: EditorConflict | null;
}

export interface BufferSnapshotRequest {
  resource: ResourceRef;
  base: DocumentVersion;
  lineEnding: "lf" | "crlf" | "none";
  content: string;
  revision: number;
}

export interface BufferSnapshotAck {
  revision: number;
  persistedAt: number;
  dirty: boolean;
}

export interface EditorSaveResult {
  outcome: "saved" | "unchanged" | "conflict";
  document: EditorDocument;
}

export interface RecoveredEditorBuffer {
  resource: ResourceRef;
  revision: number;
}

export type DiffLineKind = "context" | "added" | "removed" | "meta";

export interface DiffLine {
  kind: DiffLineKind;
  oldLine: number | null;
  newLine: number | null;
  text: string;
}

export interface DiffHunk {
  header: string;
  lines: DiffLine[];
}

export interface FileDiff {
  resource: ResourceRef;
  hunks: DiffHunk[];
  binary: boolean;
  truncated: boolean;
}

export interface ProjectLayoutRequest {
  projectId: string;
  tabs: WorkbenchTab[];
  activeTabId: string | null;
  selectedRootId: string | null;
  selectedResource: ResourceRef | null;
}

export interface RenameChatRequest {
  projectId: string;
  chatId: string;
  title: string;
  currentLayout: ProjectLayoutRequest;
}

export interface ChatLifecycleRequest {
  projectId: string;
  chatId: string;
  currentLayout: ProjectLayoutRequest;
}

export interface ArchivedChatSummary {
  id: string;
  title: string;
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

export type HarnessInstallState =
  "available" | "notInstalled" | "installing" | "unavailable";

interface HarnessDefinitionBase {
  id: string;
  name: string;
  description: string;
  source: "builtIn" | "registry" | "custom" | "development";
}

export type HarnessDefinition = HarnessDefinitionBase &
  (
    | {
        transport: "legacySimulator";
        installState: HarnessInstallState;
        bindingId: null;
      }
    | {
        transport: "acp";
        installState: "available";
        bindingId: string;
      }
    | {
        transport: "acp";
        installState: Exclude<HarnessInstallState, "available">;
        bindingId: string | null;
      }
  );

export interface ModelOption {
  id: string;
  name: string;
  description?: string;
}

export interface ChatMessage {
  id: string;
  sessionId: string;
  sequence: number;
  role: "user" | "assistant" | "system";
  text: string;
  createdAt: number;
}

export interface SimulatorTurnResult {
  sessionId: string;
  messages: ChatMessage[];
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

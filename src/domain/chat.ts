import type { RepositoryRef } from "./repository";
import type { ResourceRef } from "./resource";
import type { AgentDiffRef } from "./agentDiff";

export type TabKind =
  | "chat"
  | "file"
  | "diff"
  | "terminal"
  | "search"
  | "problems"
  | "output"
  | "settings"
  | "browser";

export type SessionStatus =
  | "idle"
  | "working"
  | "waiting"
  | "recovering"
  | "failed"
  | "complete"
  | "closed";

export interface WorkbenchTab {
  id: string;
  kind: TabKind;
  title: string;
  position: number;
  pinned: boolean;
  dirty: boolean;
  favicon?: string;
  openerTabId?: string;
  status?: SessionStatus;
  updatedAt?: number;
  finishedAt?: number;
  resource?: ResourceRef;
  repository?: RepositoryRef;
  diffRef?: AgentDiffRef;
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

export interface ChatDeleteRequest {
  projectId: string;
  chatId: string;
}

export interface PinChatRequest {
  projectId: string;
  chatId: string;
}

export interface MoveChatRequest {
  projectId: string;
  chatId: string;
  targetProjectId: string;
  currentLayout: ProjectLayoutRequest;
}

export interface ArchivedChatSummary {
  id: string;
  title: string;
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

export interface ProjectLayoutRequest {
  projectId: string;
  tabs: WorkbenchTab[];
  activeTabId: string | null;
  selectedRootId: string | null;
  selectedResource: ResourceRef | null;
}

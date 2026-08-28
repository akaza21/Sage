import type {
  AgentRailChat,
  ArchivedChatSummary,
  ProjectListItem,
} from "../domain/workbench";

export const MAX_AGENT_CHAT_MOVE_DESTINATIONS = 32;
export const MAX_AGENT_RAIL_PROJECTS = 64;
export const MAX_AGENT_RAIL_CHATS_PER_PROJECT = 8;
export const MAX_AGENT_RAIL_PIN_IDS = 64;
export const MAX_AGENT_RAIL_ARCHIVED_PER_PROJECT = 64;
export const LOCAL_PROJECT_NAME = "Local";

export interface AgentRailProjectGroup {
  id: string;
  name: string;
  rootCount: number;
  chats: AgentRailChat[];
  archived: ArchivedChatSummary[];
  truncated: boolean;
}

function admittedProjectId(value: unknown): string | null {
  return typeof value === "string" && value.length > 0 && value.length <= 512
    ? value
    : null;
}

export function admitAgentRailChat(
  value: Partial<AgentRailChat> | null | undefined,
): AgentRailChat | null {
  const id = admittedProjectId(value?.id);
  if (!id || typeof value?.title !== "string") return null;
  const position =
    typeof value.position === "number" && Number.isFinite(value.position)
      ? value.position
      : 0;
  return {
    id,
    title: value.title,
    pinned: value.pinned === true,
    position,
    ...(value.status ? { status: value.status } : {}),
    ...(typeof value.updatedAt === "number" && Number.isFinite(value.updatedAt)
      ? { updatedAt: value.updatedAt }
      : {}),
  };
}

function admitRailChats(
  chats: ReadonlyArray<Partial<AgentRailChat>> | undefined,
  limit: number,
): { chats: AgentRailChat[]; overflow: boolean } {
  const next: AgentRailChat[] = [];
  let overflow = false;
  for (const chat of chats ?? []) {
    const admitted = admitAgentRailChat(chat);
    if (!admitted) continue;
    if (next.length >= limit) {
      overflow = true;
      break;
    }
    next.push(admitted);
  }
  return { chats: next, overflow };
}

export function groupAgentRailProjects(input: {
  projects: ReadonlyArray<ProjectListItem>;
  currentProjectId: string | null;
  currentChats: ReadonlyArray<Partial<AgentRailChat>>;
}): AgentRailProjectGroup[] {
  const groups: AgentRailProjectGroup[] = [];
  const limit = Math.min(input.projects.length, MAX_AGENT_RAIL_PROJECTS);
  for (let index = 0; index < limit; index += 1) {
    const project = input.projects[index];
    const id = admittedProjectId(project?.id);
    if (!id || !project) continue;
    const current = id === input.currentProjectId;
    const source = current
      ? admitRailChats(input.currentChats, 64)
      : admitRailChats(project.railChats, MAX_AGENT_RAIL_CHATS_PER_PROJECT);
    const name =
      typeof project.name === "string" && project.name.length > 0
        ? project.name.slice(0, 64)
        : "Project";
    groups.push({
      id,
      name,
      rootCount: project.rootCount,
      chats: source.chats.sort(compareAgentRailChats),
      archived: current
        ? []
        : admitArchivedChats(
            project.railArchived,
            MAX_AGENT_RAIL_ARCHIVED_PER_PROJECT,
          ),
      truncated: current
        ? false
        : source.overflow || project.railChatsTruncated === true,
    });
  }
  return groups;
}

function admitArchivedChats(
  chats: ReadonlyArray<Partial<ArchivedChatSummary>> | undefined,
  limit: number,
): ArchivedChatSummary[] {
  const next: ArchivedChatSummary[] = [];
  for (const chat of chats ?? []) {
    const id = admittedProjectId(chat?.id);
    if (
      !id ||
      typeof chat.title !== "string" ||
      chat.title.length === 0 ||
      chat.title.length > 1024
    ) {
      continue;
    }
    if (next.length >= limit) break;
    next.push({ id, title: chat.title });
  }
  return next;
}

export function collectAgentRailPinIds(
  groups: ReadonlyArray<AgentRailProjectGroup>,
  currentProjectId: string | null,
): string[] {
  const ids: string[] = [];
  const current = groups.find((group) => group.id === currentProjectId);
  if (current) {
    for (const chat of current.chats) {
      if (ids.length >= MAX_AGENT_RAIL_PIN_IDS) return ids;
      ids.push(chat.id);
    }
  }
  for (const group of groups) {
    if (group.id === currentProjectId) continue;
    for (const chat of group.chats) {
      if (ids.length >= MAX_AGENT_RAIL_PIN_IDS) return ids;
      ids.push(chat.id);
    }
  }
  return ids;
}

export type AgentChatMoveKind = "folder" | "local";

export interface AgentChatMoveDestination {
  id: string;
  name: string;
  kind: AgentChatMoveKind;
}

export function agentChatMoveKind(rootCount: number): AgentChatMoveKind {
  return rootCount === 0 ? "local" : "folder";
}

export function agentChatMoveDestinations(
  projects: ReadonlyArray<{ id: string; name: string; rootCount: number }>,
  currentProjectId: string | null,
): AgentChatMoveDestination[] {
  const next: AgentChatMoveDestination[] = [];
  const limit = Math.min(projects.length, MAX_AGENT_CHAT_MOVE_DESTINATIONS);
  for (let index = 0; index < limit; index += 1) {
    const project = projects[index];
    if (
      !project ||
      typeof project.id !== "string" ||
      project.id.length === 0 ||
      project.id.length > 512 ||
      project.id === currentProjectId
    ) {
      continue;
    }
    const name =
      typeof project.name === "string" && project.name.length > 0
        ? project.name.slice(0, 64)
        : "Project";
    next.push({
      id: project.id,
      name,
      kind: agentChatMoveKind(project.rootCount),
    });
    if (next.length >= MAX_AGENT_CHAT_MOVE_DESTINATIONS) break;
  }
  return next;
}

export function compareAgentRailChats(
  left: { pinned: boolean; position: number },
  right: { pinned: boolean; position: number },
): number {
  if (left.pinned !== right.pinned) return left.pinned ? -1 : 1;
  return left.position - right.position;
}

import type {
  AgentRailChat,
  ArchivedChatSummary,
  ProjectListItem,
} from "../domain/workbench";
import { compareAgentRailViewChats } from "./agentRailStatusView";
import { MAX_AGENT_RAIL_PROJECTS } from "../domain/agentRailLimits";

export { MAX_AGENT_RAIL_PROJECTS } from "../domain/agentRailLimits";

export const MAX_AGENT_CHAT_MOVE_DESTINATIONS = 32;
export const MAX_AGENT_RAIL_CHATS_PER_PROJECT = 8;
export const MAX_AGENT_RAIL_CURRENT_CHATS = 64;
export const MAX_AGENT_RAIL_PIN_IDS = 64;
export const MAX_AGENT_RAIL_RECENT_CHATS = 64;
export const MAX_AGENT_RAIL_ARCHIVED_PER_PROJECT = 64;
export const UNKNOWN_PROJECT_NAME = "Unknown project";

export interface AgentRailProjectGroup {
  id: string;
  name: string;
  rootCount: number;
  chats: AgentRailChat[];
  archived: ArchivedChatSummary[];
  truncated: boolean;
  totalChatCount?: number;
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
  const source = chats ?? [];
  const scanLimit = Math.min(source.length, limit + 1);
  for (let index = 0; index < scanLimit; index += 1) {
    const chat = source[index];
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
  const currentProjectId = admittedProjectId(input.currentProjectId);
  let currentProjectIndex = -1;
  const projectScanLimit = Math.min(
    input.projects.length,
    MAX_AGENT_RAIL_PROJECTS + 1,
  );
  for (let index = 0; index < projectScanLimit; index += 1) {
    if (input.projects[index]?.id === currentProjectId) {
      currentProjectIndex = index;
      break;
    }
  }
  const projectIndexes = Array.from(
    { length: Math.min(input.projects.length, MAX_AGENT_RAIL_PROJECTS) },
    (_, index) => index,
  );
  if (currentProjectIndex >= MAX_AGENT_RAIL_PROJECTS) {
    projectIndexes[projectIndexes.length - 1] = currentProjectIndex;
  }
  let currentIncluded = false;
  for (const index of projectIndexes) {
    const project = input.projects[index];
    const id = admittedProjectId(project?.id);
    if (!id || !project) continue;
    const current = id === currentProjectId;
    currentIncluded ||= current;
    const source = current
      ? admitRailChats(input.currentChats, MAX_AGENT_RAIL_CURRENT_CHATS)
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
      truncated:
        source.overflow || (!current && project.railChatsTruncated === true),
    });
  }

  if (
    !currentIncluded &&
    currentProjectId &&
    groups.length < MAX_AGENT_RAIL_PROJECTS
  ) {
    const source = admitRailChats(
      input.currentChats,
      MAX_AGENT_RAIL_CURRENT_CHATS,
    );
    if (source.chats.length > 0) {
      groups.push({
        id: currentProjectId,
        name: UNKNOWN_PROJECT_NAME,
        rootCount: 0,
        chats: source.chats.sort(compareAgentRailChats),
        archived: [],
        truncated: source.overflow,
      });
    }
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

export interface AgentRailChatItem {
  projectId: string;
  projectName: string;
  projectChatCount: number;
  chat: AgentRailChat;
}

export interface AgentRailSections {
  pinned: AgentRailChatItem[];
  projects: AgentRailProjectGroup[];
  recent: AgentRailChatItem[];
}

export function deriveAgentRailSections(
  groups: ReadonlyArray<AgentRailProjectGroup>,
  chatCountByProjectId?: Readonly<Record<string, number>>,
): AgentRailSections {
  const pinned: AgentRailChatItem[] = [];
  const recentCandidates: AgentRailChatItem[] = [];
  const projects: AgentRailProjectGroup[] = [];

  for (const group of groups) {
    const projectChats: AgentRailChat[] = [];
    const projectChatCount =
      chatCountByProjectId?.[group.id] ??
      group.totalChatCount ??
      group.chats.length;
    for (const chat of group.chats) {
      const item = {
        projectId: group.id,
        projectName: group.name,
        projectChatCount,
        chat,
      };
      if (chat.pinned && pinned.length < MAX_AGENT_RAIL_PIN_IDS) {
        pinned.push(item);
      } else {
        projectChats.push(chat);
      }
      if (
        typeof chat.updatedAt === "number" &&
        Number.isFinite(chat.updatedAt) &&
        chat.updatedAt > 0
      )
        recentCandidates.push(item);
    }
    projects.push({
      ...group,
      chats: projectChats,
      totalChatCount: projectChatCount,
    });
  }

  recentCandidates.sort((left, right) =>
    compareAgentRailViewChats(left.chat, right.chat, "recency"),
  );
  return {
    pinned,
    projects,
    recent: recentCandidates.slice(0, MAX_AGENT_RAIL_RECENT_CHATS),
  };
}

export function filterAgentRailGroups(
  groups: ReadonlyArray<AgentRailProjectGroup>,
  normalizedQuery: string,
): AgentRailProjectGroup[] {
  if (!normalizedQuery) return groups.slice();
  const next: AgentRailProjectGroup[] = [];
  for (const group of groups) {
    const groupMatches = group.name
      .toLocaleLowerCase()
      .includes(normalizedQuery);
    const chats = groupMatches
      ? group.chats
      : group.chats.filter((chat) =>
          chat.title.toLocaleLowerCase().includes(normalizedQuery),
        );
    const archived = groupMatches
      ? group.archived
      : group.archived.filter((chat) =>
          chat.title.toLocaleLowerCase().includes(normalizedQuery),
        );
    if (chats.length === 0 && archived.length === 0 && !groupMatches) continue;
    next.push({
      ...group,
      chats,
      archived,
      truncated: group.truncated,
    });
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

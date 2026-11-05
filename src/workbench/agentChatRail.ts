import { MAX_AGENT_RAIL_PROJECTS } from "../domain/agentRailLimits";
import type {
  AgentRailChat,
  ArchivedChatSummary,
  ProjectListItem,
} from "../domain/workbench";
import {
  compareAgentRailViewChats,
  type AgentRailOrdering,
} from "./agentRailStatusView";

export { MAX_AGENT_RAIL_PROJECTS } from "../domain/agentRailLimits";

export const MAX_AGENT_CHAT_MOVE_DESTINATIONS = 32;
// A foreign project arrives with its first eight chats by last update and its
// first eight by manual order, so either sort previews the right chats.
export const MAX_AGENT_RAIL_CHATS_PER_PROJECT = 16;
// A project lists its first few chats until "Show more" (Codex shows five).
export const MAX_AGENT_RAIL_PROJECT_PREVIEW_CHATS = 5;
export const MAX_AGENT_RAIL_CURRENT_CHATS = 64;
export const MAX_AGENT_RAIL_PIN_IDS = 64;
// Keep the ready rail small; retain a bounded page for explicit expansion.
export const MAX_AGENT_RAIL_RECENT_CHATS = 8;
export const MAX_AGENT_RAIL_RECENT_CANDIDATES = 64;
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
    ...(typeof value.finishedAt === "number" &&
    Number.isSafeInteger(value.finishedAt) &&
    value.finishedAt > 0
      ? { finishedAt: value.finishedAt }
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
  /** Every unpinned chat in the chosen order, for "In one list". */
  all: AgentRailChatItem[];
  /**
   * When "In one list" cannot hold every chat (its own cap, or a project the
   * rail admitted only in part), the folder where Chat search continues.
   */
  allOverflowProjectId: string | null;
}

export function deriveAgentRailSections(
  groups: ReadonlyArray<AgentRailProjectGroup>,
  chatCountByProjectId?: Readonly<Record<string, number>>,
  ordering: AgentRailOrdering = "recency",
  activeChatId: string | null = null,
): AgentRailSections {
  const pinned: AgentRailChatItem[] = [];
  const unpinned: AgentRailChatItem[] = [];
  const recentCandidates: AgentRailChatItem[] = [];
  const projects: AgentRailProjectGroup[] = [];

  for (const group of groups) {
    const projectChats: AgentRailChat[] = [];
    const projectChatCount =
      chatCountByProjectId?.[group.id] ??
      group.totalChatCount ??
      group.chats.length;
    const chats =
      ordering === "manual"
        ? group.chats
            .slice()
            .sort((left, right) => left.position - right.position)
        : group.chats;
    for (const chat of chats) {
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
        unpinned.push(item);
      }
      if (
        !chat.pinned &&
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
  // Manual order keeps the projects' order and each project's own order.
  if (ordering === "recency") {
    unpinned.sort((left, right) =>
      compareAgentRailViewChats(left.chat, right.chat, ordering),
    );
  }
  const all = unpinned.slice(0, MAX_AGENT_RAIL_RECENT_CANDIDATES);
  // The open chat always stays in the list, even past its cap.
  const active = unpinned.find((item) => item.chat.id === activeChatId);
  if (active && !all.includes(active)) all[all.length - 1] = active;
  return {
    pinned,
    projects,
    recent: recentCandidates.slice(0, MAX_AGENT_RAIL_RECENT_CANDIDATES),
    all,
    allOverflowProjectId:
      unpinned[MAX_AGENT_RAIL_RECENT_CANDIDATES]?.projectId ??
      groups.find((group) => group.truncated)?.id ??
      null,
  };
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

/**
 * The chats a project shows: its first few in the chosen order, or every
 * admitted chat once expanded. The open chat always stays in view.
 */
export function visibleProjectChats(
  chats: ReadonlyArray<AgentRailChat>,
  activeId: string | null,
  ordering: AgentRailOrdering,
  expanded: boolean,
): AgentRailChat[] {
  const sorted = chats
    .slice()
    .sort((left, right) => compareAgentRailViewChats(left, right, ordering));
  if (expanded) return sorted;
  const visible = sorted.slice(0, MAX_AGENT_RAIL_PROJECT_PREVIEW_CHATS);
  const active = sorted.find((chat) => chat.id === activeId);
  if (active && !visible.includes(active)) visible[visible.length - 1] = active;
  return visible;
}

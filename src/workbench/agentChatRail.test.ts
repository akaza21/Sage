import { describe, expect, it } from "vitest";

import {
  MAX_AGENT_RAIL_CHATS_PER_PROJECT,
  MAX_AGENT_RAIL_PIN_IDS,
  MAX_AGENT_RAIL_RECENT_CHATS,
  agentChatMoveDestinations,
  collectAgentRailPinIds,
  compareAgentRailChats,
  deriveAgentRailSections,
  filterAgentRailGroups,
  groupAgentRailProjects,
} from "./agentChatRail";

describe("agent chat rail", () => {
  it("lists other projects for a move popover and marks 0-root as Local", () => {
    expect(
      agentChatMoveDestinations(
        [
          { id: "project-a", name: "Sage", rootCount: 1 },
          { id: "project-b", name: "Work", rootCount: 2 },
          { id: "project-local", name: "Local", rootCount: 0 },
          { id: "", name: "junk", rootCount: 1 },
        ],
        "project-a",
      ),
    ).toEqual([
      { id: "project-b", name: "Work", kind: "folder" },
      { id: "project-local", name: "Local", kind: "local" },
    ]);
  });

  it("pins admitted chats above unpinned ones", () => {
    const chats = [
      { pinned: false, position: 0 },
      { pinned: true, position: 2 },
      { pinned: false, position: 1 },
    ];
    expect([...chats].sort(compareAgentRailChats)).toEqual([
      { pinned: true, position: 2 },
      { pinned: false, position: 0 },
      { pinned: false, position: 1 },
    ]);
  });

  it("groups chats under every project and prefers live tabs for the current one", () => {
    expect(
      groupAgentRailProjects({
        projects: [
          {
            id: "cross",
            name: "Cross",
            rootCount: 1,
            lastOpenedAt: 2,
            railChats: [
              { id: "chat:x", title: "Research", pinned: false, position: 0 },
            ],
          },
          {
            id: "sage",
            name: "Sage",
            rootCount: 1,
            lastOpenedAt: 1,
            railChats: [
              { id: "chat:stale", title: "Stale", pinned: false, position: 0 },
            ],
          },
          { id: "", name: "junk", rootCount: 1, lastOpenedAt: 0 },
        ],
        currentProjectId: "sage",
        currentChats: [
          { id: "chat:s", title: "Continue prior", pinned: true, position: 0 },
          { id: "chat:s2", title: "Second", pinned: false, position: 1 },
        ],
      }),
    ).toEqual([
      {
        id: "cross",
        name: "Cross",
        rootCount: 1,
        chats: [
          { id: "chat:x", title: "Research", pinned: false, position: 0 },
        ],
        archived: [],
        truncated: false,
      },
      {
        id: "sage",
        name: "Sage",
        rootCount: 1,
        chats: [
          { id: "chat:s", title: "Continue prior", pinned: true, position: 0 },
          { id: "chat:s2", title: "Second", pinned: false, position: 1 },
        ],
        archived: [],
        truncated: false,
      },
    ]);
  });

  it("lists archived chats on foreign folders only", () => {
    const groups = groupAgentRailProjects({
      projects: [
        {
          id: "cross",
          name: "Cross",
          rootCount: 1,
          lastOpenedAt: 2,
          railChats: [],
          railArchived: [{ id: "chat:old", title: "Research" }],
        },
        {
          id: "sage",
          name: "Sage",
          rootCount: 1,
          lastOpenedAt: 1,
          railArchived: [{ id: "chat:ignored", title: "Ignored" }],
        },
      ],
      currentProjectId: "sage",
      currentChats: [
        { id: "chat:s", title: "Continue prior", pinned: false, position: 0 },
      ],
    });
    expect(groups[0]?.archived).toEqual([
      { id: "chat:old", title: "Research" },
    ]);
    expect(groups[1]?.archived).toEqual([]);
  });

  it("derives capped pinned and recent sections and falls pin overflow back to projects", () => {
    const groups = [
      {
        id: "sage",
        name: "Sage",
        rootCount: 1,
        chats: [
          ...Array.from({ length: MAX_AGENT_RAIL_PIN_IDS + 2 }, (_, index) => ({
            id: `pin:${index}`,
            title: `Pinned ${index}`,
            pinned: true,
            position: index,
            updatedAt: 100 + index,
          })),
          ...Array.from(
            { length: MAX_AGENT_RAIL_RECENT_CHATS + 2 },
            (_, index) => ({
              id: `recent:${index}`,
              title: `Recent ${index}`,
              pinned: false,
              position: index,
              updatedAt: index,
            }),
          ),
        ],
        archived: [],
        truncated: false,
      },
    ];

    const sections = deriveAgentRailSections(groups);
    expect(sections.pinned).toHaveLength(MAX_AGENT_RAIL_PIN_IDS);
    expect(sections.projects[0]?.chats).toEqual([
      expect.objectContaining({ id: "pin:64", pinned: true }),
      expect.objectContaining({ id: "pin:65", pinned: true }),
      ...Array.from({ length: 10 }, (_, index) =>
        expect.objectContaining({ id: `recent:${index}`, pinned: false }),
      ),
    ]);
    expect(sections.recent).toHaveLength(MAX_AGENT_RAIL_RECENT_CHATS);
    expect(sections.recent.map((item) => item.chat.id)).toEqual([
      "recent:9",
      "recent:8",
      "recent:7",
      "recent:6",
      "recent:5",
      "recent:4",
      "recent:3",
      "recent:2",
    ]);
    expect(sections.recent.some((item) => item.chat.pinned)).toBe(false);
    expect(
      deriveAgentRailSections([
        {
          ...groups[0],
          chats: [
            {
              id: "unknown-time",
              title: "Unknown time",
              pinned: false,
              position: 0,
            },
          ],
        },
      ]).recent,
    ).toEqual([]);
  });

  it("filters admitted rail groups locally while preserving matching project caps", () => {
    const groups = [
      {
        id: "cross",
        name: "Cross",
        rootCount: 1,
        chats: [
          { id: "chat:x", title: "Research", pinned: false, position: 0 },
        ],
        archived: [{ id: "old:x", title: "Archived research" }],
        truncated: true,
      },
      {
        id: "sage",
        name: "Sage",
        rootCount: 1,
        chats: [
          { id: "chat:s", title: "Implement rail", pinned: false, position: 0 },
        ],
        archived: [],
        truncated: false,
      },
    ];

    expect(filterAgentRailGroups(groups, "research")).toEqual([groups[0]]);
    expect(filterAgentRailGroups(groups, "cross")).toEqual([groups[0]]);
    expect(filterAgentRailGroups(groups, "archived")).toEqual([
      {
        ...groups[0],
        chats: [],
        archived: [{ id: "old:x", title: "Archived research" }],
      },
    ]);
    expect(filterAgentRailGroups(groups, "missing")).toEqual([]);
  });

  it("hoists the current project into the bounded project window", () => {
    const projects = Array.from({ length: 65 }, (_, index) => ({
      id: `project:${index}`,
      name: `Project ${index}`,
      rootCount: 1,
      lastOpenedAt: index,
    }));
    const groups = groupAgentRailProjects({
      projects,
      currentProjectId: "project:64",
      currentChats: [
        { id: "chat:current", title: "Current", pinned: false, position: 0 },
      ],
    });
    expect(groups).toHaveLength(64);
    expect(groups.at(-1)).toMatchObject({
      id: "project:64",
      name: "Project 64",
      chats: [expect.objectContaining({ id: "chat:current" })],
    });
    expect(groups.some((group) => group.id === "project:63")).toBe(false);
    expect(groups.some((group) => group.name === "Unknown project")).toBe(
      false,
    );
  });

  it("groups orphaned current chats under Unknown project", () => {
    expect(
      groupAgentRailProjects({
        projects: [],
        currentProjectId: "missing",
        currentChats: [
          { id: "chat:o", title: "Orphan", pinned: false, position: 0 },
        ],
      }),
    ).toEqual([
      {
        id: "missing",
        name: "Unknown project",
        rootCount: 0,
        chats: [{ id: "chat:o", title: "Orphan", pinned: false, position: 0 }],
        archived: [],
        truncated: false,
      },
    ]);
  });

  it("caps foreign project chats and pins the current project first", () => {
    const overflow = Array.from(
      { length: MAX_AGENT_RAIL_CHATS_PER_PROJECT + 2 },
      (_, index) => ({
        id: `chat:${index}`,
        title: `Chat ${index}`,
        pinned: false,
        position: index,
      }),
    );
    const groups = groupAgentRailProjects({
      projects: [
        {
          id: "work",
          name: "Work",
          rootCount: 1,
          lastOpenedAt: 1,
          railChats: overflow,
          railChatsTruncated: true,
        },
        {
          id: "local",
          name: "Local",
          rootCount: 0,
          lastOpenedAt: 2,
        },
      ],
      currentProjectId: "local",
      currentChats: [
        { id: "chat:local", title: "Local chat", pinned: false, position: 0 },
      ],
    });
    expect(groups[0]?.chats).toHaveLength(MAX_AGENT_RAIL_CHATS_PER_PROJECT);
    expect(groups[0]?.truncated).toBe(true);
    expect(collectAgentRailPinIds(groups, "local")).toEqual([
      "chat:local",
      ...overflow
        .slice(0, MAX_AGENT_RAIL_CHATS_PER_PROJECT)
        .map((chat) => chat.id),
    ]);
  });
});

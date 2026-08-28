import { describe, expect, it } from "vitest";

import {
  MAX_AGENT_RAIL_CHATS_PER_PROJECT,
  agentChatMoveDestinations,
  collectAgentRailPinIds,
  compareAgentRailChats,
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

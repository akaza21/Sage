import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { AgentRecentChats } from "./AgentRecentChats";
import { agentRailViewPrefs } from "./agentRailViewPrefs";
afterEach(cleanup);
it("renders eight recent tasks and expands only on an explicit request", () => {
  const items = Array.from({ length: 20 }, (_, index) => ({
    projectId: "project",
    projectName: "Project",
    projectChatCount: 20,
    chat: {
      id: `chat-${index}`,
      title: `Task ${index}`,
      pinned: false,
      position: index,
      updatedAt: 100,
    },
  }));
  render(
    <AgentRecentChats
      items={items}
      currentProjectId="project"
      prefs={agentRailViewPrefs()}
    />,
  );
  expect(
    screen.getAllByRole("button", { name: /^Open recent chat Task / }),
  ).toHaveLength(8);
  fireEvent.click(
    screen.getByRole("button", { name: "Show more recent tasks" }),
  );
  expect(
    screen.getAllByRole("button", { name: /^Open recent chat Task / }),
  ).toHaveLength(16);
  fireEvent.click(
    screen.getByRole("button", { name: "Show more recent tasks" }),
  );
  expect(
    screen.getAllByRole("button", { name: /^Open recent chat Task / }),
  ).toHaveLength(20);
  expect(
    screen.queryByRole("button", { name: "Show more recent tasks" }),
  ).toBeNull();
});

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { projectChatLiveSnapshot } from "../features/chat/acp/chatLiveSnapshot";
import { sessionRailStatusLabel } from "../features/chat/acp/sessionRail";
import { AgentTaskStatus } from "./AgentTaskStatus";
import {
  resetAgentRailInteractions,
  setAgentRailApproval,
} from "./agentRailInteractions";
import { useSideChatState } from "./sideChatState";

afterEach(() => {
  cleanup();
  useSideChatState.setState({ sides: [] });
  resetAgentRailInteractions();
});

it("uses the rail's parent status while a side chat runs", () => {
  useSideChatState.setState({
    sides: [
      {
        id: "side-1",
        projectId: "project-1",
        parentChatId: "chat-1",
        running: true,
      },
    ],
  });
  render(
    <AgentTaskStatus
      chatId="chat-1"
      snapshot={projectChatLiveSnapshot("chat-1", null)}
      status="complete"
      harnessStatus="Live"
    />,
  );
  expect(screen.getByRole("status")).toHaveTextContent(
    sessionRailStatusLabel("working"),
  );
  expect(screen.queryByText("Live")).toBeNull();
});

it("uses the rail's approval status before the turn reports waiting", () => {
  setAgentRailApproval("chat-1", true);
  render(
    <AgentTaskStatus
      chatId="chat-1"
      snapshot={projectChatLiveSnapshot("chat-1", null)}
      status="working"
    />,
  );
  expect(screen.getByRole("status")).toHaveTextContent("Awaiting approval");
});

it("keeps a harness problem behind its icon until opened", () => {
  render(
    <AgentTaskStatus
      chatId="chat-1"
      snapshot={null}
      harnessStatus="Disconnected"
    />,
  );
  expect(screen.getByRole("status")).toHaveAccessibleName("Chat status: Idle");
  const explanation = screen.getByText("Disconnected", { selector: "p" });
  expect(explanation).not.toBeVisible();
  fireEvent.click(screen.getByLabelText("Harness status: Disconnected"));
  expect(explanation).toBeVisible();
});

it("shows no harness control for a neutral harness status", () => {
  const view = render(
    <AgentTaskStatus chatId="chat-1" snapshot={null} harnessStatus="Ready" />,
  );
  expect(screen.queryByLabelText("Harness status: Ready")).toBeNull();
  view.rerender(
    <AgentTaskStatus chatId="chat-1" snapshot={null} harnessStatus="Live" />,
  );
  expect(screen.queryByLabelText("Harness status: Live")).toBeNull();
});

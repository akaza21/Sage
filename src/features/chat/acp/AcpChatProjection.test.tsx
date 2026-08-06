import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { AcpChatPage, AcpItem, TurnProjection } from "../../../domain/acp";
import { AcpChatProjection } from "./AcpChatProjection";

afterEach(cleanup);

function textItem(): Extract<AcpItem, { kind: "text" }> {
  return {
    id: "item-text",
    kind: "text",
    origin: "primary",
    payload: { role: "assistant", text: "Normalized answer" },
  } as unknown as Extract<AcpItem, { kind: "text" }>;
}

function page(state: TurnProjection["state"]): AcpChatPage {
  return {
    projectId: "project-1",
    chatId: "chat-1",
    title: "Durable chat",
    projectionRevision: "12",
    currentSessionId: "session-1",
    sessions: [],
    nextCursor: null,
    turns: [
      {
        id: "turn-1",
        sessionId: "session-1",
        runtimeEpoch: "2",
        ordinal: 1,
        state,
        revision: "3",
        createdAt: 1,
        updatedAt: 2,
        terminalAt: state === "completed" ? 2 : null,
        items: [textItem()],
        activities: [],
      } as unknown as TurnProjection,
    ],
  } as unknown as AcpChatPage;
}

describe("AcpChatProjection", () => {
  it("exposes every Turn transition through the frozen atomic status semantics", () => {
    render(<AcpChatProjection page={page("waitingUser")} active />);
    const status = screen.getByRole("status", { name: "Waiting for user" });
    expect(status).toHaveTextContent("Waiting for user");
    expect(status).toHaveAttribute("aria-live", "polite");
    expect(status).toHaveAttribute("aria-atomic", "true");
  });

  it("renders lifecycle state and normalized items", () => {
    render(<AcpChatProjection page={page("waitingModel")} active />);
    expect(screen.getByRole("status")).toHaveTextContent("Waiting for model");
    expect(screen.getByText("Normalized answer")).toBeInTheDocument();
  });

  it("keeps cancellation out of the transcript projection", () => {
    render(<AcpChatProjection page={page("completed")} active />);
    expect(screen.getByRole("status")).toHaveTextContent("Completed");
    expect(screen.queryByRole("button", { name: /cancel turn/i })).toBeNull();
  });

  it("fully unmounts an inactive heavy projection", () => {
    const { container } = render(
      <AcpChatProjection page={page("running")} active={false} />,
    );
    expect(container).toBeEmptyDOMElement();
  });

  it("exposes earlier history only as an explicit accessible action", () => {
    const loadEarlier = vi.fn();
    const history = page("completed");
    history.nextCursor = "cursor" as never;
    render(
      <AcpChatProjection
        page={history}
        active
        hasEarlier
        onLoadEarlier={loadEarlier}
      />,
    );
    fireEvent.click(
      screen.getByRole("button", { name: "Load earlier history" }),
    );
    expect(loadEarlier).toHaveBeenCalledTimes(1);
  });
});

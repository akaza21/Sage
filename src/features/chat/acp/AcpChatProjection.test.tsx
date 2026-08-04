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
  it("renders lifecycle state and exposes a keyboard-accessible cancel intent", () => {
    const onCancel = vi.fn();
    render(
      <AcpChatProjection
        page={page("waitingModel")}
        active
        cancellableTurnId={"turn-1" as never}
        onCancelTurn={onCancel}
      />,
    );
    expect(screen.getByRole("status")).toHaveTextContent("Waiting for model");
    expect(screen.getByText("Normalized answer")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Cancel turn 1" }));
    expect(onCancel).toHaveBeenCalledWith("turn-1");
  });

  it("does not expose cancel for a terminal Turn", () => {
    render(<AcpChatProjection page={page("completed")} active />);
    expect(screen.getByRole("status")).toHaveTextContent("Completed");
    expect(screen.queryByRole("button", { name: /cancel turn/i })).toBeNull();
  });

  it("does not expose an interactive cancel without a wired handler", () => {
    render(
      <AcpChatProjection
        page={page("running")}
        active
        cancellableTurnId={"turn-1" as never}
      />,
    );
    expect(screen.queryByRole("button", { name: /cancel turn/i })).toBeNull();
  });

  it("does not expose cancel for a different or ambiguous active Turn", () => {
    render(
      <AcpChatProjection
        page={page("running")}
        active
        cancellableTurnId={"turn-other" as never}
        onCancelTurn={vi.fn()}
      />,
    );
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

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";

import { browserOperationFixture } from "../../domain/browserOperation.fixture";
import { BrowserAgentPresence } from "./BrowserAgentPresence";

afterEach(cleanup);

function renderPresence(userControl: boolean) {
  const handlers = {
    onStop: vi.fn(),
    onTakeOver: vi.fn(),
    onHandBack: vi.fn(),
  };
  render(
    <BrowserAgentPresence
      operation={{ ...browserOperationFixture, phase: "started" }}
      userControl={userControl}
      chatTitle="Research chat"
      {...handlers}
    />,
  );
  return handlers;
}

it("names the chat using the tab and offers Take over and Stop", () => {
  const { onStop, onTakeOver, onHandBack } = renderPresence(false);
  expect(screen.getByRole("status")).toHaveTextContent(
    "Agent has a browser action pending · Research chat",
  );
  fireEvent.click(screen.getByRole("button", { name: "Take over" }));
  expect(onTakeOver).toHaveBeenCalledOnce();
  fireEvent.click(screen.getByRole("button", { name: "Stop agent" }));
  expect(onStop).toHaveBeenCalledOnce();
  expect(onHandBack).not.toHaveBeenCalled();
});

it("offers Hand back while the user controls the tab", () => {
  const { onTakeOver, onHandBack } = renderPresence(true);
  expect(screen.getByRole("status")).toHaveTextContent(
    "You're in control · Research chat",
  );
  expect(screen.queryByRole("button", { name: "Take over" })).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Hand back" }));
  expect(onHandBack).toHaveBeenCalledOnce();
  expect(onTakeOver).not.toHaveBeenCalled();
  expect(screen.getByRole("button", { name: "Stop agent" })).toBeEnabled();
});

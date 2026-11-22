import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { acpProjectionStoreFor } from "../../app/acpProjectionRegistry";
import { pinAgentChatOwner } from "../../app/agentChatOwner";
import type { AcpChatPage } from "../../domain/acp";
import { AgentToolsContext } from "../../workbench/AgentToolsProvider";
import { ComposerSectionRows as ComposerAddActions } from "./ComposerSectionMenu";

afterEach(() => {
  cleanup();
  pinAgentChatOwner(null);
});

function renderActions(chatAvailable: boolean, nativeSessionImport = true) {
  pinAgentChatOwner("another-chat");
  acpProjectionStoreFor("chat-import").setState({
    page: {
      currentSessionId: "session-1",
      sessions: [{ id: "session-1", capabilities: { nativeSessionImport } }],
    } as unknown as AcpChatPage,
  });
  const launch = vi.fn();
  const onClose = vi.fn();
  render(
    <AgentToolsContext
      value={{ launch, chatAvailable, terminalAvailable: false }}
    >
      <ComposerAddActions
        boundChatId="chat-import"
        sessionConfig={null}
        pending={false}
        disabled={false}
        onClose={onClose}
        onSetSessionMode={vi.fn()}
        onSetSessionConfigOption={vi.fn()}
      />
    </AgentToolsContext>,
  );
  return { launch, onClose };
}

it("opens the owning chat's session picker when another chat is focused", () => {
  const { launch, onClose } = renderActions(true);
  fireEvent.click(
    screen.getByRole("menuitem", { name: "Open a Codex CLI session" }),
  );
  expect(onClose).toHaveBeenCalled();
  expect(launch).toHaveBeenCalledWith("sessionImport");
});

it("offers no picker where no Chat owns the launch", () => {
  renderActions(false);
  expect(
    screen.queryByRole("menuitem", { name: "Open a Codex CLI session" }),
  ).toBeNull();
});

it("offers no picker when the chat's current session cannot import", () => {
  renderActions(true, false);
  expect(
    screen.queryByRole("menuitem", { name: "Open a Codex CLI session" }),
  ).toBeNull();
});

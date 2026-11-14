import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { useChatAccessDrafts } from "../../app/chatAccessDrafts";
import { useSessionAccessModeStore } from "../../app/sessionAccessModeStore";
import type {
  AcpChatPage,
  ClientIntentId,
  CuratedBindingId,
  CuratedModelId,
  ProjectGeneration,
  ProjectionRevision,
  RuntimeEpoch,
  SageAgentSessionId,
  SageChatId,
  SageProjectId,
} from "../../domain/acp";
import {
  agentSessionFixture,
  capabilitySetFixture,
} from "../../domain/acpProjection.fixture";
import type { SessionAccessModeCapability } from "../../domain/sessionAccessMode";
import {
  createAcpActionController,
  type AcpActionCommands,
} from "./acp/acpActionController";
import { ComposerAccessControl } from "./ComposerAccessControl";

const port = vi.hoisted(() => ({
  get: vi.fn(),
  set: vi.fn(),
  prepare: vi.fn(),
  grant: vi.fn(),
}));
vi.mock("../../ports/chatAccessChanges", () => ({
  chatAccessChanges: () => port,
}));

const codex: SessionAccessModeCapability[] = [
  {
    id: "readOnly",
    label: "Read only",
    description: "The agent reads files and asks before changes.",
    fallback: true,
  },
  {
    id: "ask",
    label: "Ask each time",
    description: "Review each permission request.",
  },
  {
    id: "workspaceWrite",
    label: "Workspace write",
    description: "The agent edits files in this workspace.",
  },
  {
    id: "fullAccess",
    label: "Full access",
    description: "Everything runs without asking.",
  },
];

/** A brand-new chat: its session does not exist yet. */
function renderNewChat(chatId?: string) {
  return render(
    <ComposerAccessControl
      harnessName="Fixture"
      bindingId="codex-acp"
      chatId={chatId}
      capabilities={codex}
      sessionConfig={null}
      open
      onToggle={() => {}}
      onDismiss={() => {}}
      disabled={false}
    />,
  );
}
const chip = () => screen.getByRole("button", { name: "Access options" });
const option = (name: string) => screen.findByRole("menuitemradio", { name });

beforeEach(() => {
  for (const fn of Object.values(port)) fn.mockReset();
  useSessionAccessModeStore.setState({ modes: {}, ready: true, error: null });
  useChatAccessDrafts.setState({ drafts: {} });
});
afterEach(cleanup);

it("lets a new chat choose its access before the first message", async () => {
  renderNewChat("chat-1");
  expect(chip()).toHaveTextContent("Read only");
  const ask = await option("Ask each time");
  expect(ask).toHaveAttribute("aria-disabled", "false");
  fireEvent.click(ask);
  await waitFor(() => expect(chip()).toHaveTextContent("Ask each time"));
  expect(useChatAccessDrafts.getState().drafts).toEqual({
    "chat-1": { bindingId: "codex-acp", mode: "ask" },
  });
  // Nothing reaches the native side until the session exists.
  expect(port.set).not.toHaveBeenCalled();
  // The harness default for other and new chats is untouched.
  expect(useSessionAccessModeStore.getState().modes).toEqual({});
});

it("confirms an elevated draft the way a live chat's menu does", async () => {
  renderNewChat("chat-1");
  fireEvent.click(await option("Workspace write"));
  expect(
    await screen.findByRole("heading", { name: "Switch to Workspace write?" }),
  ).toBeVisible();
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  expect(useChatAccessDrafts.getState().drafts).toEqual({});
  expect(chip()).toHaveTextContent("Read only");

  cleanup();
  renderNewChat("chat-1");
  fireEvent.click(await option("Workspace write"));
  fireEvent.click(await screen.findByRole("button", { name: "Switch access" }));
  await waitFor(() => expect(chip()).toHaveTextContent("Workspace write"));
  expect(port.set).not.toHaveBeenCalled();
});

/** The chat's first send: its session starts, then the prompt is sent. */
function firstSend(chatId: string, order: string[]) {
  const session = agentSessionFixture({
    id: "session-new" as SageAgentSessionId,
    bindingId: "codex-acp" as CuratedBindingId,
    modelId: "model-a" as CuratedModelId,
    state: "active",
    residency: "hot",
    runtimeEpoch: "1" as RuntimeEpoch,
    capabilities: capabilitySetFixture({ protocolVersion: "1" }),
  });
  const page = (revision: string, started: boolean): AcpChatPage => ({
    projectId: "project-a" as SageProjectId,
    chatId: chatId as SageChatId,
    title: "Chat",
    projectionRevision: revision as ProjectionRevision,
    currentSessionId: started ? session.id : null,
    sessions: started ? [session] : [],
    turns: [],
    nextCursor: null,
  });
  let current = page("1", false);
  const commands = {
    startSession: vi.fn(async () => {
      order.push("start");
      return { projection: page("2", true) };
    }),
    sendTurn: vi.fn(async () => {
      order.push("send");
      return { projection: page("3", true) };
    }),
  } as unknown as AcpActionCommands;
  let intent = 0;
  return createAcpActionController({
    commands,
    getContext: () => ({
      scope: {
        projectId: "project-a" as SageProjectId,
        projectGeneration: "3" as ProjectGeneration,
        chatId: chatId as SageChatId,
      },
      ownershipEpoch: 1,
      commandProjectGeneration: "11" as ProjectGeneration,
      page: current,
    }),
    applyAuthoritativePage: (_scope, authoritative) => {
      current = authoritative;
      return "accepted";
    },
    restoreComposer: () => {},
    onStateChange: () => {},
    createIntentId: () => `intent-${++intent}` as ClientIntentId,
  }).submit(
    "hello",
    [],
    "codex-acp" as CuratedBindingId,
    "model-a" as CuratedModelId,
  );
}

it("drafts Full access behind the same warning, and grants it natively before the first send", async () => {
  renderNewChat("chat-1");
  fireEvent.click(await option("Full access"));
  expect(
    await screen.findByRole("heading", {
      name: "Allow full access in this chat?",
    }),
  ).toBeVisible();
  expect(
    screen.getByText(/run commands without asking, with no filesystem sandbox/),
  ).toBeVisible();
  // Nothing is asked of the native side until the session exists.
  expect(port.prepare).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Allow full access" }));
  await waitFor(() => expect(chip()).toHaveTextContent("Full access"));
  expect(useChatAccessDrafts.getState().drafts).toEqual({
    "chat-1": { bindingId: "codex-acp", mode: "fullAccess" },
  });
  expect(port.prepare).not.toHaveBeenCalled();

  const order: string[] = [];
  port.prepare.mockImplementation(async (sessionId: string, mode: string) => {
    order.push(`prepare ${sessionId} ${mode}`);
    return { confirmToken: "token-1" };
  });
  port.grant.mockImplementation(
    async (sessionId: string, mode: string, token: string) => {
      order.push(`grant ${sessionId} ${mode} ${token}`);
    },
  );
  await expect(firstSend("chat-1", order)).resolves.toBe("applied");
  expect(order).toEqual([
    "start",
    "prepare session-new fullAccess",
    "grant session-new fullAccess token-1",
    "send",
  ]);
  expect(port.set).not.toHaveBeenCalled();
  expect(useChatAccessDrafts.getState().drafts).toEqual({});
});

it("keeps a drafted Full access unset when its warning is cancelled", async () => {
  renderNewChat("chat-1");
  fireEvent.click(await option("Full access"));
  fireEvent.click(await screen.findByRole("button", { name: "Cancel" }));
  expect(useChatAccessDrafts.getState().drafts).toEqual({});
  expect(chip()).toHaveTextContent("Read only");
});

it("clears the draft when the chat goes back to its default", async () => {
  useChatAccessDrafts.setState({
    drafts: { "chat-1": { bindingId: "codex-acp", mode: "ask" } },
  });
  renderNewChat("chat-1");
  expect(chip()).toHaveTextContent("Ask each time");
  fireEvent.click(await option("Read only"));
  await waitFor(() => expect(chip()).toHaveTextContent("Read only"));
  expect(useChatAccessDrafts.getState().drafts).toEqual({});
});

it("shows another harness's draft as nothing for this one", async () => {
  useChatAccessDrafts.setState({
    drafts: { "chat-1": { bindingId: "claude-agent-acp", mode: "ask" } },
  });
  renderNewChat("chat-1");
  expect(chip()).toHaveTextContent("Read only");
});

it("offers nothing to choose without a chat to hold the draft", async () => {
  renderNewChat();
  const ask = await option("Ask each time");
  expect(ask).toHaveAttribute("aria-disabled", "true");
  fireEvent.click(ask);
  expect(useChatAccessDrafts.getState().drafts).toEqual({});
});

import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  draftChatAccess,
  useChatAccessDrafts,
} from "../../../app/chatAccessDrafts";
import { useChatEffortDraft } from "../../../app/chatEffortDraft";
import { useChatAccessStore } from "../../../app/sessionAccessActions";
import type {
  AcpChatPage,
  AgentSessionProjection,
  ClientIntentId,
  CuratedBindingId,
  CuratedModelId,
  ProjectGeneration,
  ProjectionRevision,
  RuntimeEpoch,
  SageAgentSessionId,
  SageChatId,
  SageProjectId,
} from "../../../domain/acp";
import {
  agentSessionFixture,
  capabilitySetFixture,
} from "../../../domain/acpProjection.fixture";
import {
  createAcpActionController,
  type AcpActionCommands,
} from "./acpActionController";

const port = vi.hoisted(() => ({
  get: vi.fn(),
  set: vi.fn(),
  prepare: vi.fn(),
  grant: vi.fn(),
}));
vi.mock("../../../ports/chatAccessChanges", () => ({
  chatAccessChanges: () => port,
}));

const scope = {
  projectId: "project-a" as SageProjectId,
  projectGeneration: "3" as ProjectGeneration,
  chatId: "chat-a" as SageChatId,
};
const bindingId = "codex" as CuratedBindingId;
const modelId = "model-a" as CuratedModelId;

function session(): AgentSessionProjection {
  return agentSessionFixture({
    id: "session-new" as SageAgentSessionId,
    bindingId,
    modelId,
    state: "active",
    residency: "hot",
    runtimeEpoch: "1" as RuntimeEpoch,
    capabilities: capabilitySetFixture({ protocolVersion: "1" }),
  });
}

function page(revision: string, current: AgentSessionProjection | null) {
  return {
    projectId: scope.projectId,
    chatId: scope.chatId,
    title: "Chat",
    projectionRevision: revision as ProjectionRevision,
    currentSessionId: current?.id ?? null,
    sessions: current ? [current] : [],
    turns: [],
    nextCursor: null,
  } satisfies AcpChatPage;
}

/** A brand-new chat whose first message starts its session. */
function newChat() {
  let current: AcpChatPage = page("1", null);
  const order: string[] = [];
  const restored: string[] = [];
  const errors: string[] = [];
  let intent = 0;
  const commands = {
    startSession: vi.fn(async () => {
      order.push("start");
      return { projection: page("2", session()) };
    }),
    sendTurn: vi.fn(async () => {
      order.push("send");
      return { projection: page("3", session()) };
    }),
  } as unknown as AcpActionCommands;
  port.set.mockImplementation(async (sessionId: string, mode: string) => {
    order.push(`access ${sessionId} ${mode}`);
    return null;
  });
  const controller = createAcpActionController({
    commands,
    getContext: () => ({
      scope,
      ownershipEpoch: 1,
      commandProjectGeneration: "11" as ProjectGeneration,
      page: current,
    }),
    applyAuthoritativePage: (_scope, authoritative) => {
      current = authoritative;
      return "accepted";
    },
    restoreComposer: (_context, prompt) => restored.push(prompt),
    onStateChange: (state) => {
      if (state.error) errors.push(state.error.message);
    },
    createIntentId: () => `intent-${++intent}` as ClientIntentId,
  });
  return { controller, commands, order, restored, errors };
}

beforeEach(() => {
  for (const fn of Object.values(port)) fn.mockReset();
  useChatAccessDrafts.setState({ drafts: {} });
  useChatEffortDraft.setState({ draft: null });
  useChatAccessStore.setState({ modes: {}, error: null });
});

describe("access drafted before a chat's session starts", () => {
  it("is given to the new session before its first prompt is sent", async () => {
    draftChatAccess(scope.chatId, { bindingId, mode: "workspaceWrite" });
    const test = newChat();

    await expect(
      test.controller.submit("hello", [], bindingId, modelId),
    ).resolves.toBe("applied");

    expect(test.order).toEqual([
      "start",
      "access session-new workspaceWrite",
      "send",
    ]);
    // Spent once: the session's own access menu takes over from here.
    expect(useChatAccessDrafts.getState().drafts).toEqual({});
    expect(useChatAccessStore.getState().modes).toEqual({
      "session-new": "workspaceWrite",
    });
  });

  it("sends nothing when the session refuses the drafted access", async () => {
    draftChatAccess(scope.chatId, { bindingId, mode: "workspaceWrite" });
    const test = newChat();
    port.set.mockRejectedValue(new Error("not offered"));
    port.get.mockResolvedValue(null);

    await expect(
      test.controller.submit("hello", [], bindingId, modelId),
    ).resolves.toBe("rejected");

    expect(test.commands.sendTurn).not.toHaveBeenCalled();
    expect(test.restored).toEqual(["hello"]);
    expect(test.errors.at(-1)).toBe(
      "The chosen access was not given to this chat, so nothing was sent. Check its access and send again.",
    );
  });

  it("gives a permissive draft only through a confirmed native grant", async () => {
    draftChatAccess(scope.chatId, { bindingId, mode: "fullAccess" });
    port.prepare.mockResolvedValue({ confirmToken: "token-1" });
    port.grant.mockRejectedValue(new Error("not allowed"));
    port.get.mockResolvedValue(null);
    const test = newChat();

    await expect(
      test.controller.submit("hello", [], bindingId, modelId),
    ).resolves.toBe("rejected");

    expect(port.set).not.toHaveBeenCalled();
    expect(port.prepare).toHaveBeenCalledWith("session-new", "fullAccess");
    expect(port.grant).toHaveBeenCalledWith(
      "session-new",
      "fullAccess",
      "token-1",
    );
    expect(test.commands.sendTurn).not.toHaveBeenCalled();
  });

  it("drops a draft made for another harness and sends under the default", async () => {
    draftChatAccess(scope.chatId, { bindingId: "claude", mode: "plan" });
    const test = newChat();

    await expect(
      test.controller.submit("hello", [], bindingId, modelId),
    ).resolves.toBe("applied");

    expect(test.order).toEqual(["start", "send"]);
    expect(useChatAccessDrafts.getState().drafts).toEqual({});
  });
});

describe("a thinking level drafted before a chat's session starts", () => {
  it("goes with the session start for the same binding and model", async () => {
    useChatEffortDraft.setState({
      draft: { chatId: scope.chatId, bindingId, modelId, effort: "high" },
    });
    const test = newChat();

    await expect(
      test.controller.submit("hello", [], bindingId, modelId),
    ).resolves.toBe("applied");

    expect(test.commands.startSession).toHaveBeenCalledWith(
      expect.objectContaining({ bindingId, modelId, effort: "high" }),
    );
    // Held until the session exists, so a failed start sends it again.
    expect(useChatEffortDraft.getState().draft?.effort).toBe("high");
  });

  it("is not sent for another model", async () => {
    useChatEffortDraft.setState({
      draft: {
        chatId: scope.chatId,
        bindingId,
        modelId: "model-b",
        effort: "high",
      },
    });
    const test = newChat();

    await test.controller.submit("hello", [], bindingId, modelId);

    expect(test.commands.startSession).toHaveBeenCalledWith(
      expect.not.objectContaining({ effort: expect.anything() }),
    );
  });
});

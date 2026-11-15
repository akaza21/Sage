import { describe, expect, it, vi } from "vitest";

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
} from "../../../domain/acp";
import { agentSessionFixture } from "../../../domain/acpProjection.fixture";
import type { AcpCommandList } from "../../../services/acpCommandsDesktop";
import {
  createAcpActionController,
  type AcpActionCommands,
} from "./acpActionController";
import type { SlashCommandPort } from "../slashCommandTurn";
import { setPickedCommand } from "../slashCommandPicks";

const scope = {
  projectId: "project-a" as SageProjectId,
  projectGeneration: "3" as ProjectGeneration,
  chatId: "chat-a" as SageChatId,
};
const bindingId = "binding-a" as CuratedBindingId;
const modelId = "model-a" as CuratedModelId;
const live: AcpChatPage = {
  projectId: scope.projectId,
  chatId: scope.chatId,
  title: "Chat",
  projectionRevision: "10" as ProjectionRevision,
  currentSessionId: "session-a" as SageAgentSessionId,
  sessions: [
    agentSessionFixture({
      id: "session-a" as SageAgentSessionId,
      bindingId,
      modelId,
      state: "active",
      residency: "hot",
      runtimeEpoch: "2" as RuntimeEpoch,
    }),
  ],
  turns: [],
  nextCursor: null,
};

function harness(list: AcpCommandList | Error) {
  let intent = 0;
  const cleared: string[] = [];
  const restored: string[] = [];
  const errors: string[] = [];
  const commands = {
    sendTurn: vi.fn(async () => ({ projection: live })),
  } as unknown as AcpActionCommands;
  const slashCommands: SlashCommandPort = {
    listCommands: vi.fn(async () => {
      if (list instanceof Error) throw list;
      return list;
    }),
    runCommand: vi.fn(async () => ({ projection: live })),
  };
  const controller = createAcpActionController({
    commands,
    slashCommands,
    getContext: () => ({
      scope,
      ownershipEpoch: 1,
      commandProjectGeneration: "11" as ProjectGeneration,
      page: live,
    }),
    applyAuthoritativePage: () => "accepted",
    clearComposer: (_context, prompt) => cleared.push(prompt),
    restoreComposer: (_context, prompt) => restored.push(prompt),
    onStateChange: (state) => {
      if (state.error) errors.push(state.error.message);
    },
    createIntentId: () => `intent-${++intent}` as ClientIntentId,
  });
  return { controller, commands, slashCommands, cleared, restored, errors };
}

const review = {
  revision: 4,
  commands: [
    {
      name: "review",
      description: "Review",
      inputHint: "PR",
      source: "harness" as const,
    },
  ],
};

describe("slash commands on the send path", () => {
  it("sends an advertised command through the typed route with the live revision", async () => {
    const test = harness(review);
    await expect(
      test.controller.submit("/review  42\nmore", [], bindingId, modelId),
    ).resolves.toBe("applied");
    expect(test.commands.sendTurn).not.toHaveBeenCalled();
    expect(test.slashCommands.runCommand).toHaveBeenCalledWith({
      ...scope,
      projectGeneration: "11",
      expectedProjectionRevision: "10",
      clientIntentId: "intent-3",
      sessionId: "session-a",
      runtimeEpoch: "2",
      expectedCommandsRevision: 4,
      name: "review",
      input: "42\nmore",
    });
    expect(test.cleared).toEqual(["/review  42\nmore"]);
    setPickedCommand(scope.chatId, "review");
    await test.controller.submit("/review", [], bindingId, modelId);
    setPickedCommand(scope.chatId, null);
    expect(
      vi.mocked(test.slashCommands.runCommand).mock.calls[1]![0],
    ).not.toHaveProperty("input");
  });

  it("keeps unadvertised or context-carrying text on the plain prompt path", async () => {
    const test = harness(review);
    await test.controller.submit("/unknown thing", [], bindingId, modelId);
    await test.controller.submit("plain /review", [], bindingId, modelId);
    const quote = {
      kind: "transcriptQuote" as const,
      ordinal: 1,
      sourceTurnId: "t",
      sourceItemId: "i",
      text: "quote",
      startOffset: 0,
      endOffset: 5,
    };
    await test.controller.submit("/review 1", [quote], bindingId, modelId);
    expect(test.slashCommands.runCommand).not.toHaveBeenCalled();
    expect(
      vi
        .mocked(test.commands.sendTurn)
        .mock.calls.map(
          (call) => (call as unknown as [{ prompt: string }])[0].prompt,
        ),
    ).toEqual(["/unknown thing", "plain /review", "/review 1"]);
  });

  it("refuses a picked command the session no longer offers and keeps the draft", async () => {
    const test = harness({ revision: 5, commands: [] });
    setPickedCommand(scope.chatId, "review");
    await expect(
      test.controller.submit("/review 42", [], bindingId, modelId),
    ).resolves.toBe("rejected");
    setPickedCommand(scope.chatId, null);
    expect(test.commands.sendTurn).not.toHaveBeenCalled();
    expect(test.slashCommands.runCommand).not.toHaveBeenCalled();
    expect(test.errors.at(-1)).toBe(
      "This session no longer offers /review, so nothing was sent.",
    );
    expect(test.restored).toEqual(["/review 42"]);
    expect(test.cleared).toEqual([]);
  });

  it("refuses a picked command when the list cannot be read, but sends typed text", async () => {
    const test = harness(new Error("unavailable"));
    setPickedCommand(scope.chatId, "review");
    await expect(
      test.controller.submit("/review", [], bindingId, modelId),
    ).resolves.toBe("rejected");
    setPickedCommand(scope.chatId, null);
    await expect(
      test.controller.submit("/review", [], bindingId, modelId),
    ).resolves.toBe("applied");
    expect(test.commands.sendTurn).toHaveBeenCalledTimes(1);
  });
});

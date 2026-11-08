import { describe, expect, it, vi } from "vitest";

import type { AcpProjectionScope } from "../../../app/acpProjectionStore";
import type {
  AcpCapabilitySet,
  AcpChatPage,
  AgentSessionProjection,
  ClientIntentId,
  CuratedBindingId,
  CuratedModelId,
  EntityRevision,
  ProjectGeneration,
  ProjectionRevision,
  RuntimeEpoch,
  SageAgentSessionId,
  SageChatId,
  SageProjectId,
  SageTurnId,
  TurnProjection,
} from "../../../domain/acp";
import { AcpDesktopError } from "../../../services/acpDesktop";
import {
  type AcpActionCommands,
  type AcpActionContext,
  cancellableTurnId,
  createAcpActionController,
} from "./acpActionController";
import { createClientIntentFactory } from "./clientIntent";
import {
  agentSessionFixture,
  capabilitySetFixture,
} from "../../../domain/acpProjection.fixture";

const scope: AcpProjectionScope = {
  projectId: "project-a" as SageProjectId,
  projectGeneration: "3" as ProjectGeneration,
  chatId: "chat-a" as SageChatId,
};
const commandProjectGeneration = "11" as ProjectGeneration;
const bindingId = "binding-a" as CuratedBindingId;
const modelId = "model-a" as CuratedModelId;

const capabilities: AcpCapabilitySet = capabilitySetFixture({
  protocolVersion: "1",
  resumeSession: true,
  loadSession: true,
});

function session(
  residency: AgentSessionProjection["residency"],
  runtimeEpoch: string | null,
  nextRuntimeEpoch: string | null,
): AgentSessionProjection {
  return agentSessionFixture({
    id: "session-a" as SageAgentSessionId,
    bindingId,
    modelId,
    state: residency === "cold" ? "ready" : "active",
    residency,
    runtimeEpoch: runtimeEpoch as RuntimeEpoch | null,
    nextRuntimeEpoch: nextRuntimeEpoch as RuntimeEpoch | null,
    capabilities,
  });
}

function turn(
  id: string,
  ordinal: number,
  state: TurnProjection["state"],
  runtimeEpoch = "1",
): TurnProjection {
  return {
    id: id as SageTurnId,
    sessionId: "session-a" as SageAgentSessionId,
    runtimeEpoch: runtimeEpoch as RuntimeEpoch,
    ordinal,
    state,
    revision: "7" as EntityRevision,
    createdAt: 1,
    updatedAt: 2,
    terminalAt: null,
    items: [],
    activities: [],
    browserOperations: [],
    browserOperationsCollapsed: 0,
  };
}

function page(
  revision: string,
  current: AgentSessionProjection | null,
  turns: TurnProjection[] = [],
): AcpChatPage {
  return {
    projectId: scope.projectId,
    chatId: scope.chatId,
    title: "Chat",
    projectionRevision: revision as ProjectionRevision,
    currentSessionId: current?.id ?? null,
    sessions: current ? [current] : [],
    turns,
    nextCursor: null,
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((accept, decline) => {
    resolve = accept;
    reject = decline;
  });
  return { promise, resolve, reject };
}

async function nextMicrotask(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
}

function harness(
  initialPage: AcpChatPage,
  settleAccessMode?: () => Promise<boolean>,
) {
  let context: AcpActionContext | null = {
    scope,
    ownershipEpoch: 1,
    commandProjectGeneration,
    page: initialPage,
  };
  const applied: AcpChatPage[] = [];
  const restored: Array<{ prompt: string; attachedContext: unknown[] }> = [];
  const cleared: Array<{ prompt: string; attachedContext: unknown[] }> = [];
  const states: Array<{
    scope: AcpProjectionScope | null;
    origin: "chat" | "mode";
    busy: boolean;
    error: AcpDesktopError | null;
  }> = [];
  let intent = 0;
  const commands = {
    startSession: vi.fn(),
    activateSession: vi.fn(),
    sendTurn: vi.fn(),
    cancelTurn: vi.fn(),
    steerTurn: vi.fn(),
    closeSession: vi.fn(),
    endSession: vi.fn(),
    setSessionMode: vi.fn(),
    setSessionConfigOption: vi.fn(),
    queueSessionConfigChoice: vi.fn(),
  } as unknown as AcpActionCommands;
  const controller = createAcpActionController({
    commands,
    getContext: () => context,
    ...(settleAccessMode ? { settleAccessMode } : {}),
    applyAuthoritativePage: (_scope, authoritative) => {
      applied.push(authoritative);
      if (context) context = { ...context, page: authoritative };
      return "accepted";
    },
    clearComposer: (_context, prompt, attachedContext) =>
      cleared.push({ prompt, attachedContext: [...attachedContext] }),
    restoreComposer: (_context, prompt, attachedContext) =>
      restored.push({ prompt, attachedContext: [...attachedContext] }),
    onStateChange: (state) =>
      states.push({
        scope: state.scope,
        origin: state.origin,
        busy: state.busy,
        error: state.error,
      }),
    createIntentId: () => `intent-${++intent}` as ClientIntentId,
  });
  return {
    controller,
    commands,
    applied,
    restored,
    cleared,
    states,
    setContext: (value: AcpActionContext | null) => {
      context = value;
    },
    getContext: () => context,
  };
}

describe("ACP action controller", () => {
  it("sends annotation-only context while clearing the original empty draft", async () => {
    const test = harness(page("10", session("hot", "2", null)));
    vi.mocked(test.commands.sendTurn).mockResolvedValue({
      projection: page("11", session("hot", "2", null)),
    });
    const quote = {
      kind: "transcriptQuote" as const,
      ordinal: 1,
      sourceTurnId: "t",
      sourceItemId: "i",
      text: "quote",
      startOffset: 0,
      endOffset: 5,
    };
    await expect(
      test.controller.submit("", [quote], bindingId, modelId),
    ).resolves.toBe("applied");
    expect(test.commands.sendTurn).toHaveBeenCalledWith(
      expect.objectContaining({
        prompt: "Please respond to the attached context.",
        attachedContext: [quote],
      }),
    );
    expect(test.cleared).toEqual([{ prompt: "", attachedContext: [quote] }]);
  });
  it("sends attachment-only context and still refuses an empty send", async () => {
    const test = harness(page("10", session("hot", "2", null)));
    vi.mocked(test.commands.sendTurn).mockResolvedValue({
      projection: page("11", session("hot", "2", null)),
    });
    await expect(
      test.controller.submit("  ", [], bindingId, modelId),
    ).resolves.toBe("rejected");
    expect(test.commands.sendTurn).not.toHaveBeenCalled();
    const image = {
      kind: "image" as const,
      artifactId: "00000000-0000-4000-8000-000000000000",
      mediaType: "image/png" as const,
      width: 1,
      height: 1,
      byteLength: 1,
      sha256: "a".repeat(64),
      label: "sample.png",
    };
    await expect(
      test.controller.submit("", [image], bindingId, modelId),
    ).resolves.toBe("applied");
    expect(test.commands.sendTurn).toHaveBeenCalledWith(
      expect.objectContaining({
        prompt: "Please respond to the attached context.",
        attachedContext: [image],
      }),
    );
  });
  it("rejects a mode that is no longer advertised before enqueueing", async () => {
    const initial = {
      ...page("10", session("hot", "2", null)),
      sessionConfig: {
        sageAgentSessionId: "session-a",
        runtimeEpoch: "2",
        configRevision: "3",
        modes: {
          currentModeId: "ask",
          availableModes: [{ id: "ask", name: "Ask" }],
        },
        configOptions: [],
      },
    };
    const test = harness(initial);

    await expect(test.controller.setSessionMode("code")).resolves.toBe(
      "rejected",
    );
    expect(test.commands.setSessionMode).not.toHaveBeenCalled();
    expect(test.states).toHaveLength(0);
  });

  it("never sends a set-mode request for the mode that is already current", async () => {
    const initial = {
      ...page("10", session("hot", "2", null)),
      sessionConfig: {
        sageAgentSessionId: "session-a",
        runtimeEpoch: "2",
        configRevision: "3",
        modes: {
          currentModeId: "ask",
          availableModes: [
            { id: "ask", name: "Ask" },
            { id: "code", name: "Code" },
          ],
        },
        configOptions: [],
      },
    };
    const test = harness(initial);

    await expect(test.controller.setSessionMode("ask")).resolves.toBe(
      "ignoredStale",
    );
    expect(test.commands.setSessionMode).not.toHaveBeenCalled();

    // A mode that becomes current while queued is also not sent.
    const pending = test.controller.setSessionMode("code");
    test.setContext({
      ...test.getContext()!,
      page: {
        ...initial,
        sessionConfig: {
          ...initial.sessionConfig,
          modes: { ...initial.sessionConfig.modes, currentModeId: "code" },
        },
      },
    });
    await expect(pending).resolves.toBe("ignoredStale");
    expect(test.commands.setSessionMode).not.toHaveBeenCalled();
  });

  it("revalidates the mode against the live config before dispatch", async () => {
    const initial = {
      ...page("10", session("hot", "2", null)),
      sessionConfig: {
        sageAgentSessionId: "session-a",
        runtimeEpoch: "2",
        configRevision: "3",
        modes: {
          currentModeId: "ask",
          availableModes: [
            { id: "ask", name: "Ask" },
            { id: "code", name: "Code" },
          ],
        },
        configOptions: [],
      },
    };
    const test = harness(initial);
    const pending = test.controller.setSessionMode("code");
    test.setContext({
      ...test.getContext()!,
      page: {
        ...initial,
        projectionRevision: "11" as ProjectionRevision,
        sessionConfig: {
          ...initial.sessionConfig,
          configRevision: "3",
          modes: {
            currentModeId: "ask",
            availableModes: [{ id: "ask", name: "Ask" }],
          },
        },
      },
    });

    await expect(pending).resolves.toBe("ignoredStale");
    expect(test.commands.setSessionMode).not.toHaveBeenCalled();
  });

  it("does not optimistically echo a mode and carries exact config authority", async () => {
    const initial = {
      ...page("10", session("hot", "2", null)),
      sessionConfig: {
        sageAgentSessionId: "session-a",
        runtimeEpoch: "2",
        configRevision: "3",
        modes: {
          currentModeId: "ask",
          availableModes: [
            { id: "ask", name: "Ask" },
            { id: "code", name: "Code" },
          ],
        },
        configOptions: [],
      },
    };
    const test = harness(initial);
    let finish!: (value: { projection: AcpChatPage }) => void;
    vi.mocked(test.commands.setSessionMode!).mockImplementation(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    const pending = test.controller.setSessionMode("code");
    await Promise.resolve();
    await Promise.resolve();
    expect(test.getContext()?.page.sessionConfig?.modes?.currentModeId).toBe(
      "ask",
    );
    expect(test.applied).toHaveLength(0);
    expect(test.commands.setSessionMode).toHaveBeenCalledWith(
      expect.objectContaining({
        modeId: "code",
        expectedConfigRevision: "3",
        expectedProjectionRevision: "10",
        sessionId: "session-a",
        runtimeEpoch: "2",
        projectGeneration: commandProjectGeneration,
      }),
    );
    finish({ projection: initial });
    expect(await pending).toBe("applied");
    expect(test.getContext()?.page.sessionConfig?.modes?.currentModeId).toBe(
      "ask",
    );
  });
  it("does not leave a mode pending after it collides with a prompt action", async () => {
    const initial = {
      ...page("10", session("hot", "2", null)),
      sessionConfig: {
        sageAgentSessionId: "session-a",
        runtimeEpoch: "2",
        configRevision: "3",
        modes: {
          currentModeId: "ask",
          availableModes: [
            { id: "ask", name: "Ask" },
            { id: "code", name: "Code" },
          ],
        },
        configOptions: [],
      },
    };
    const test = harness(initial);
    const pendingPrompt = deferred<{ projection: AcpChatPage }>();
    vi.mocked(test.commands.sendTurn).mockReturnValue(pendingPrompt.promise);

    const prompt = test.controller.submit("Prompt", [], bindingId, modelId);
    await nextMicrotask();
    await expect(test.controller.setSessionMode("code")).resolves.toBe(
      "rejected",
    );
    expect(test.states.at(-1)).toMatchObject({
      origin: "mode",
      busy: false,
      error: {
        message: "Another action is already in progress for this Chat.",
      },
    });

    pendingPrompt.resolve({ projection: initial });
    await expect(prompt).resolves.toBe("applied");
  });

  it("exposes no retained presentation snapshot after notifications", () => {
    const test = harness(page("1", null));

    expect(test.controller).not.toHaveProperty("getState");
  });

  it("serially starts, activates, and sends using each authoritative projection", async () => {
    const test = harness(page("1", null));
    const created = page("2", session("cold", null, "1"));
    const active = page("3", session("hot", "1", null));
    const sent = page("4", session("hot", "1", null), [
      turn("turn-a", 1, "running"),
    ]);
    vi.mocked(test.commands.startSession).mockResolvedValue({
      projection: created,
    });
    vi.mocked(test.commands.activateSession).mockResolvedValue({
      projection: active,
    });
    vi.mocked(test.commands.sendTurn).mockResolvedValue({ projection: sent });

    await expect(
      test.controller.submit("  hello  ", [], bindingId, modelId),
    ).resolves.toBe("applied");

    expect(test.commands.startSession).toHaveBeenCalledWith(
      expect.objectContaining({
        projectGeneration: commandProjectGeneration,
        expectedProjectionRevision: "1",
        clientIntentId: "intent-1",
      }),
    );
    expect(test.commands.activateSession).toHaveBeenCalledWith(
      expect.objectContaining({
        expectedProjectionRevision: "2",
        runtimeEpoch: "1",
        clientIntentId: "intent-2",
      }),
    );
    expect(test.commands.sendTurn).toHaveBeenCalledWith(
      expect.objectContaining({
        target: "session",
        expectedProjectionRevision: "3",
        runtimeEpoch: "1",
        prompt: "hello",
        clientIntentId: "intent-3",
      }),
    );
    expect(
      vi.mocked(test.commands.sendTurn).mock.calls[0]?.[0],
    ).not.toHaveProperty("attachedContext");
    expect(test.applied).toEqual([created, active, sent]);
    expect(test.cleared).toEqual([{ prompt: "hello", attachedContext: [] }]);
    expect(test.restored).toEqual([]);
  });

  it("carries typed attached context through send and composer clearing", async () => {
    const test = harness(page("5", session("hot", "1", null)));
    const sent = page("6", session("hot", "1", null));
    vi.mocked(test.commands.sendTurn).mockResolvedValue({ projection: sent });
    const attachedContext = [
      {
        kind: "diffLineComment" as const,
        diffRef: {
          projectId: scope.projectId,
          chatId: scope.chatId,
          sessionId: "session-a",
          runtimeEpoch: "1",
          turnId: "turn-source",
          itemId: "item-source",
          diffOrdinal: 0 as const,
          snapshotRevision: "1",
          rootId: "root-a",
          relativePath: "src/app.ts",
          contentDigest: "a".repeat(64),
        } as never,
        side: "new" as const,
        startLine: "4",
        endLine: "4",
        note: "Keep this explicit",
      },
    ];

    await expect(
      test.controller.submit("Fix it", attachedContext, bindingId, modelId),
    ).resolves.toBe("applied");
    expect(test.commands.sendTurn).toHaveBeenCalledWith(
      expect.objectContaining({ attachedContext }),
    );
    expect(test.cleared).toEqual([{ prompt: "Fix it", attachedContext }]);
  });

  it("retains a native Goal across a Goal-neutral turn mutation for the same Session epoch", async () => {
    const goal = {
      kind: "goal" as const,
      snapshot: {
        objective: "Ship safely",
        status: "active" as const,
        tokenBudget: null,
        tokensUsed: 2,
        timeUsedSeconds: 3,
        createdAt: null,
        updatedAt: null,
      },
    };
    const test = harness({ ...page("5", session("hot", "1", null)), goal });
    vi.mocked(test.commands.sendTurn).mockResolvedValue({
      projection: page("6", session("hot", "1", null)),
    });
    await expect(
      test.controller.submit("hello", [], bindingId, modelId),
    ).resolves.toBe("applied");
    expect(test.applied[0]?.goal).toEqual(goal);
  });

  it.each(["updated", "cleared"] as const)(
    "keeps an intervening %s Goal when a delayed turn mutation settles",
    async (change) => {
      const initial = {
        ...page("5", session("hot", "1", null)),
        goal: { kind: "loading" as const },
      };
      const test = harness(initial);
      const pending = deferred<{ projection: AcpChatPage }>();
      vi.mocked(test.commands.sendTurn).mockReturnValue(pending.promise);
      const submitted = test.controller.submit("hello", [], bindingId, modelId);
      await nextMicrotask();
      const latestGoal =
        change === "cleared"
          ? ({ kind: "noGoal" } as const)
          : ({
              kind: "goal",
              snapshot: {
                objective: "New Goal",
                status: "active",
                tokenBudget: null,
                tokensUsed: 1,
                timeUsedSeconds: 2,
                createdAt: null,
                updatedAt: null,
              },
            } as const);
      test.setContext({
        ...test.getContext()!,
        page: { ...initial, goal: latestGoal },
      });
      pending.resolve({ projection: page("6", session("hot", "1", null)) });
      await expect(submitted).resolves.toBe("applied");
      expect(test.applied[0]?.goal).toEqual(latestGoal);
    },
  );

  it("sends a reviewed browser page context only with an explicit submit", async () => {
    const test = harness(page("5", session("hot", "1", null)));
    const sent = page("6", session("hot", "1", null));
    vi.mocked(test.commands.sendTurn).mockResolvedValue({ projection: sent });
    const attachedContext = [
      {
        kind: "browserPage" as const,
        tabId: "browser-1",
        url: "https://example.com/article",
        title: "Example article",
        text: "Article text",
      },
    ];
    expect(test.commands.sendTurn).not.toHaveBeenCalled();
    await expect(
      test.controller.submit("Summarize", attachedContext, bindingId, modelId),
    ).resolves.toBe("applied");
    expect(test.commands.sendTurn).toHaveBeenCalledWith(
      expect.objectContaining({ attachedContext }),
    );
    expect(test.cleared).toEqual([{ prompt: "Summarize", attachedContext }]);
  });

  it("sends Annotate's browser elements with the prompt (AP-H1)", async () => {
    const test = harness(page("5", session("hot", "1", null)));
    vi.mocked(test.commands.sendTurn).mockResolvedValue({
      projection: page("6", session("hot", "1", null)),
    });
    const attachedContext = [1, 2].map((ordinal) => ({
      kind: "browserElement" as const,
      target: "element" as const,
      ordinal,
      tabId: "browser-1",
      url: "https://example.com/cart",
      title: "Cart",
      selector: "main > :nth-child(2 of button)",
      role: "button",
      name: "Place order",
      text: "Place order",
      box: { x: 4, y: 8, width: 160, height: 40 },
      comment: ordinal === 1 ? "Disable until ticked" : "",
    }));
    await expect(
      test.controller.submit("Fix", attachedContext, bindingId, modelId),
    ).resolves.toBe("applied");
    expect(test.commands.sendTurn).toHaveBeenCalledWith(
      expect.objectContaining({ prompt: "Fix", attachedContext }),
    );
  });

  it("activates a cold durable session at nextRuntimeEpoch before sending", async () => {
    const test = harness(page("5", session("cold", null, "4")));
    const active = page("6", session("hot", "4", null));
    const sent = page("7", session("hot", "4", null));
    vi.mocked(test.commands.activateSession).mockResolvedValue({
      projection: active,
    });
    vi.mocked(test.commands.sendTurn).mockResolvedValue({ projection: sent });

    await test.controller.submit("resume", [], bindingId, modelId);

    expect(test.commands.startSession).not.toHaveBeenCalled();
    expect(test.commands.activateSession).toHaveBeenCalledWith(
      expect.objectContaining({ runtimeEpoch: "4" }),
    );
    expect(test.commands.sendTurn).toHaveBeenCalledWith(
      expect.objectContaining({ runtimeEpoch: "4", prompt: "resume" }),
    );
  });

  it("holds the prompt until the pending permission mode has settled", async () => {
    const settling = deferred<boolean>();
    const test = harness(
      page("5", session("hot", "2", null)),
      () => settling.promise,
    );
    vi.mocked(test.commands.sendTurn).mockResolvedValue({
      projection: page("6", session("hot", "2", null)),
    });

    const action = test.controller.submit("gated", [], bindingId, modelId);
    await nextMicrotask();
    expect(test.commands.sendTurn).not.toHaveBeenCalled();

    settling.resolve(true);
    await action;
    expect(test.commands.sendTurn).toHaveBeenCalledWith(
      expect.objectContaining({ prompt: "gated" }),
    );
  });

  it("never dispatches under a permission mode that failed to persist", async () => {
    const test = harness(page("5", session("hot", "2", null)), () =>
      Promise.resolve(false),
    );

    await expect(
      test.controller.submit("unsafe", [], bindingId, modelId),
    ).resolves.toBe("rejected");

    expect(test.commands.sendTurn).not.toHaveBeenCalled();
    expect(test.commands.startSession).not.toHaveBeenCalled();
    expect(test.restored).toEqual([{ prompt: "unsafe", attachedContext: [] }]);
    expect(test.states.at(-2)?.error).not.toBeNull();
  });

  it("retains the source prompt when its Chat changes during session start", async () => {
    const test = harness(page("1", null));
    const starting = deferred<{ projection: AcpChatPage }>();
    vi.mocked(test.commands.startSession).mockReturnValue(starting.promise);
    const action = test.controller.submit("Keep in A", [], bindingId, modelId);
    await nextMicrotask();
    const nextScope = { ...scope, chatId: "chat-b" as SageChatId };
    test.setContext({
      scope: nextScope,
      ownershipEpoch: 1,
      commandProjectGeneration,
      page: { ...page("1", null), chatId: nextScope.chatId },
    });

    starting.resolve({ projection: page("2", session("cold", null, "1")) });

    await expect(action).resolves.toBe("ignoredStale");
    expect(test.cleared).toEqual([]);
    expect(test.restored).toEqual([]);
    expect(test.commands.activateSession).not.toHaveBeenCalled();
    expect(test.commands.sendTurn).not.toHaveBeenCalled();
  });

  it("retains the source prompt when its Chat changes during session activation", async () => {
    const test = harness(page("2", session("cold", null, "1")));
    const activating = deferred<{ projection: AcpChatPage }>();
    vi.mocked(test.commands.activateSession).mockReturnValue(
      activating.promise,
    );
    const action = test.controller.submit(
      "Keep activation prompt",
      [],
      bindingId,
      modelId,
    );
    await nextMicrotask();
    const nextScope = { ...scope, chatId: "chat-b" as SageChatId };
    test.setContext({
      scope: nextScope,
      ownershipEpoch: 1,
      commandProjectGeneration,
      page: { ...page("1", null), chatId: nextScope.chatId },
    });

    activating.resolve({
      projection: page("3", session("hot", "1", null)),
    });

    await expect(action).resolves.toBe("ignoredStale");
    expect(test.cleared).toEqual([]);
    expect(test.restored).toEqual([]);
    expect(test.commands.sendTurn).not.toHaveBeenCalled();
  });

  it.each([
    ["binding", "binding-b" as CuratedBindingId, modelId],
    ["model", bindingId, "model-b" as CuratedModelId],
  ])(
    "creates a new immutable session when the selected %s changes",
    async (_label, nextBinding, nextModel) => {
      const test = harness(page("5", session("hot", "1", null)));
      const createdSession = {
        ...session("cold", null, "2"),
        bindingId: nextBinding,
        modelId: nextModel,
      };
      const activeSession = {
        ...createdSession,
        state: "active" as const,
        residency: "hot" as const,
        runtimeEpoch: "2" as RuntimeEpoch,
        nextRuntimeEpoch: null,
      };
      vi.mocked(test.commands.startSession).mockResolvedValue({
        projection: page("6", createdSession),
      });
      vi.mocked(test.commands.activateSession).mockResolvedValue({
        projection: page("7", activeSession),
      });
      vi.mocked(test.commands.sendTurn).mockResolvedValue({
        projection: page("8", activeSession),
      });

      await expect(
        test.controller.submit("new attempt", [], nextBinding, nextModel),
      ).resolves.toBe("applied");
      expect(test.commands.startSession).toHaveBeenCalledWith(
        expect.objectContaining({
          bindingId: nextBinding,
          modelId: nextModel,
        }),
      );
      expect(test.commands.activateSession).toHaveBeenCalledWith(
        expect.objectContaining({ runtimeEpoch: "2" }),
      );
      expect(test.commands.sendTurn).toHaveBeenCalledWith(
        expect.objectContaining({ runtimeEpoch: "2" }),
      );
    },
  );

  it("does not send when session start returns a different binding or model", async () => {
    const test = harness(page("1", null));
    vi.mocked(test.commands.startSession).mockResolvedValue({
      projection: page("2", {
        ...session("cold", null, "1"),
        bindingId: "binding-other" as CuratedBindingId,
      }),
    });

    await expect(
      test.controller.submit("keep this prompt", [], bindingId, modelId),
    ).resolves.toBe("rejected");
    expect(test.commands.activateSession).not.toHaveBeenCalled();
    expect(test.commands.sendTurn).not.toHaveBeenCalled();
    expect(test.restored).toEqual([
      { prompt: "keep this prompt", attachedContext: [] },
    ]);
  });

  it.each([
    ["project", { ...scope, projectId: "project-b" as SageProjectId }],
    ["chat", { ...scope, chatId: "chat-b" as SageChatId }],
    ["generation", { ...scope, projectGeneration: "4" as ProjectGeneration }],
  ])(
    "ignores a late completion after the %s changes",
    async (_label, nextScope) => {
      const test = harness(page("5", session("hot", "1", null)));
      const pending = deferred<{ projection: AcpChatPage }>();
      vi.mocked(test.commands.sendTurn).mockReturnValue(pending.promise);
      const action = test.controller.submit("late", [], bindingId, modelId);
      await nextMicrotask();
      test.setContext({
        scope: nextScope,
        ownershipEpoch: 1,
        commandProjectGeneration: "12" as ProjectGeneration,
        page: page("1", null),
      });

      pending.resolve({ projection: page("6", session("hot", "1", null)) });
      await expect(action).resolves.toBe("ignoredStale");
      expect(test.applied).toEqual([]);
      expect(test.restored).toEqual([]);
    },
  );

  it("rejects a second same-Chat submission until the first settles, then permits an explicit follow-up", async () => {
    const test = harness(page("5", session("hot", "1", null)));
    const first = deferred<{ projection: AcpChatPage }>();
    vi.mocked(test.commands.sendTurn)
      .mockReturnValueOnce(first.promise)
      .mockResolvedValueOnce({
        projection: page("7", session("hot", "1", null)),
      });

    const actionOne = test.controller.submit("one", [], bindingId, modelId);
    const actionTwo = test.controller.submit("two", [], bindingId, modelId);
    await nextMicrotask();
    expect(test.commands.sendTurn).toHaveBeenCalledTimes(1);
    await expect(actionTwo).resolves.toBe("rejected");
    expect(test.states.at(-1)?.error?.failure).toMatchObject({
      kind: "capacity",
      resource: "queue",
      limit: 1,
    });
    expect(test.cleared).toEqual([{ prompt: "one", attachedContext: [] }]);
    expect(test.restored).toEqual([]);

    const runningTurn = turn("turn-first", 1, "running");
    first.resolve({
      projection: page("6", session("hot", "1", null), [runningTurn]),
    });
    await expect(actionOne).resolves.toBe("applied");
    await expect(
      test.controller.submit("too early", [], bindingId, modelId),
    ).resolves.toBe("rejected");
    expect(test.commands.sendTurn).toHaveBeenCalledTimes(1);
    expect(test.states.at(-1)?.error?.failure).toMatchObject({
      kind: "invalidRequest",
      message:
        "Wait for the current Turn to finish before sending a follow-up.",
    });

    test.setContext({
      scope,
      ownershipEpoch: 1,
      commandProjectGeneration,
      page: page("6", session("hot", "1", null), [
        { ...runningTurn, state: "completed", terminalAt: 3 },
      ]),
    });
    await expect(
      test.controller.submit("follow-up", [], bindingId, modelId),
    ).resolves.toBe("applied");
    expect(test.commands.sendTurn).toHaveBeenCalledTimes(2);
    expect(test.commands.sendTurn).toHaveBeenLastCalledWith(
      expect.objectContaining({
        expectedProjectionRevision: "6",
        prompt: "follow-up",
      }),
    );
  });

  it("keeps the same Chat locked across transient Project switch-back generations", async () => {
    const test = harness(page("5", session("hot", "1", null)));
    const pending = deferred<{ projection: AcpChatPage }>();
    vi.mocked(test.commands.sendTurn).mockReturnValue(pending.promise);
    const first = test.controller.submit("one", [], bindingId, modelId);
    await nextMicrotask();

    test.setContext({
      scope: {
        ...scope,
        projectGeneration: "4" as ProjectGeneration,
      },
      ownershipEpoch: 1,
      commandProjectGeneration: "12" as ProjectGeneration,
      page: page("5", session("hot", "1", null)),
    });

    await expect(
      test.controller.submit("switch-back repeat", [], bindingId, modelId),
    ).resolves.toBe("rejected");
    expect(test.commands.sendTurn).toHaveBeenCalledTimes(1);

    pending.resolve({ projection: page("6", session("hot", "1", null)) });
    await expect(first).resolves.toBe("ignoredStale");
  });

  it("does not let an old chat's deferred action block the newly active chat", async () => {
    const test = harness(page("5", session("hot", "1", null)));
    const oldPending = deferred<{ projection: AcpChatPage }>();
    const nextScope = { ...scope, chatId: "chat-b" as SageChatId };
    const nextPage = {
      ...page("1", session("hot", "1", null)),
      chatId: nextScope.chatId,
    };
    vi.mocked(test.commands.sendTurn)
      .mockReturnValueOnce(oldPending.promise)
      .mockResolvedValueOnce({
        projection: {
          ...nextPage,
          projectionRevision: "2" as ProjectionRevision,
        },
      });
    const oldAction = test.controller.submit("old", [], bindingId, modelId);
    await nextMicrotask();
    test.setContext({
      scope: nextScope,
      ownershipEpoch: 1,
      commandProjectGeneration,
      page: nextPage,
    });

    await expect(
      test.controller.submit("new", [], bindingId, modelId),
    ).resolves.toBe("applied");
    expect(test.commands.sendTurn).toHaveBeenCalledTimes(2);
    expect(test.states.at(-1)).toMatchObject({
      scope: nextScope,
      busy: false,
    });
    oldPending.resolve({
      projection: page("6", session("hot", "1", null)),
    });
    await expect(oldAction).resolves.toBe("ignoredStale");
  });

  it("reports a dispatched background failure only to its captured Chat", async () => {
    const test = harness(page("5", session("hot", "1", null)));
    const pending = deferred<{ projection: AcpChatPage }>();
    vi.mocked(test.commands.sendTurn).mockReturnValue(pending.promise);
    const action = test.controller.submit("retry in A", [], bindingId, modelId);
    await nextMicrotask();
    const nextScope = { ...scope, chatId: "chat-b" as SageChatId };
    test.setContext({
      scope: nextScope,
      ownershipEpoch: 1,
      commandProjectGeneration,
      page: { ...page("1", null), chatId: nextScope.chatId },
    });

    pending.reject(
      new AcpDesktopError({
        kind: "conflict",
        resource: "projectionRevision",
        message: "A failed in background",
      }),
    );

    await expect(action).resolves.toBe("rejected");
    expect(test.restored).toEqual([
      { prompt: "retry in A", attachedContext: [] },
    ]);
    expect(test.states.at(-1)).toMatchObject({
      scope,
      busy: false,
      error: { message: "A failed in background" },
    });
  });

  it("rejects a completion from a retired Chat ownership epoch", async () => {
    const test = harness(page("5", session("hot", "1", null)));
    const pending = deferred<{ projection: AcpChatPage }>();
    vi.mocked(test.commands.sendTurn).mockReturnValue(pending.promise);
    const action = test.controller.submit(
      "old renderer",
      [],
      bindingId,
      modelId,
    );
    await nextMicrotask();
    test.setContext({
      scope,
      ownershipEpoch: 2,
      commandProjectGeneration,
      page: page("5", session("hot", "1", null)),
    });

    pending.resolve({
      projection: page("6", session("hot", "1", null)),
    });

    await expect(action).resolves.toBe("ignoredStale");
    expect(test.applied).toEqual([]);
  });

  it("exposes and cancels only the exact active Turn intent", async () => {
    const activeTurn = turn("turn-active", 2, "waitingTool");
    const current = page("8", session("hot", "1", null), [
      { ...turn("turn-old", 1, "completed"), terminalAt: 2 },
      activeTurn,
    ]);
    const test = harness(current);
    vi.mocked(test.commands.cancelTurn).mockResolvedValue({
      projection: page("9", session("hot", "1", null), [
        { ...activeTurn, state: "cancelling" },
      ]),
    });

    expect(cancellableTurnId(current)).toBe("turn-active");
    await expect(
      test.controller.cancel("turn-old" as SageTurnId),
    ).resolves.toBe("rejected");
    await expect(test.controller.cancel(activeTurn.id)).resolves.toBe(
      "applied",
    );
    expect(test.commands.cancelTurn).toHaveBeenCalledOnce();
    expect(test.commands.cancelTurn).toHaveBeenCalledWith(
      expect.objectContaining({
        turnId: "turn-active",
        expectedTurnState: "waitingTool",
        expectedTurnRevision: "7",
        runtimeEpoch: "1",
      }),
    );
  });

  it("steers only the currently scoped live Turn with its exact revision", async () => {
    const activeTurn = turn("turn-active", 2, "waitingTool");
    const activeSession = agentSessionFixture({
      ...session("hot", "1", null),
      capabilities: capabilitySetFixture({ activeTurnSteer: true }),
    });
    const current = page("8", activeSession, [
      { ...turn("turn-old", 1, "completed"), terminalAt: 2 },
      activeTurn,
    ]);
    const test = harness(current);
    vi.mocked(test.commands.steerTurn!).mockResolvedValue({
      projection: page("9", activeSession, [activeTurn]),
    });

    await expect(test.controller.steer(" Change direction ", [])).resolves.toBe(
      "applied",
    );
    expect(test.commands.steerTurn).toHaveBeenCalledExactlyOnceWith({
      ...scope,
      projectGeneration: commandProjectGeneration,
      expectedProjectionRevision: "8",
      clientIntentId: "intent-1",
      sessionId: "session-a",
      runtimeEpoch: "1",
      turnId: "turn-active",
      expectedTurnRevision: "7",
      prompt: "Change direction",
    });
  });

  it("steers a queued attachment-only message with the attached-context prompt", async () => {
    const activeTurn = turn("turn-active", 2, "running");
    const activeSession = agentSessionFixture({
      ...session("hot", "1", null),
      capabilities: capabilitySetFixture({ activeTurnSteer: true }),
    });
    const test = harness(page("8", activeSession, [activeTurn]));
    vi.mocked(test.commands.steerTurn!).mockResolvedValue({
      projection: page("9", activeSession, [activeTurn]),
    });
    const quote = {
      kind: "transcriptQuote" as const,
      ordinal: 1,
      sourceTurnId: "t",
      sourceItemId: "i",
      text: "quote",
      startOffset: 0,
      endOffset: 5,
    };
    await expect(test.controller.steer("  ", [])).resolves.toBe("rejected");
    // A queued attachment-only message has empty text.
    await expect(test.controller.steer("", [quote])).resolves.toBe("applied");
    expect(test.commands.steerTurn).toHaveBeenCalledExactlyOnceWith(
      expect.objectContaining({
        prompt: "Please respond to the attached context.",
        attachedContext: [quote],
      }),
    );
  });

  it("rejects an unavailable or changed Steer target without dispatching", async () => {
    const activeTurn = turn("turn-active", 2, "running");
    const activeSession = agentSessionFixture({
      ...session("hot", "1", null),
      capabilities: capabilitySetFixture({ activeTurnSteer: true }),
    });
    const test = harness(page("8", activeSession, [activeTurn]));
    const pending = test.controller.steer("Change direction", []);
    test.setContext({
      scope,
      ownershipEpoch: 1,
      commandProjectGeneration,
      page: page("9", activeSession, [
        { ...activeTurn, state: "completed", terminalAt: 3 },
      ]),
    });
    await expect(pending).resolves.toBe("rejected");
    expect(test.commands.steerTurn).not.toHaveBeenCalled();

    const unavailable = harness(
      page("8", session("hot", "1", null), [activeTurn]),
    );
    await expect(
      unavailable.controller.steer("Change direction", []),
    ).resolves.toBe("rejected");
    expect(unavailable.commands.steerTurn).not.toHaveBeenCalled();
  });

  it("keeps Steer rejected when native dispatch rejects it", async () => {
    const activeTurn = turn("turn-active", 1, "running");
    const activeSession = agentSessionFixture({
      ...session("hot", "1", null),
      capabilities: capabilitySetFixture({ activeTurnSteer: true }),
    });
    const test = harness(page("8", activeSession, [activeTurn]));
    vi.mocked(test.commands.steerTurn!).mockRejectedValue(
      new AcpDesktopError({
        kind: "conflict",
        resource: "turnRevision",
        message: "The active Turn changed.",
      }),
    );

    await expect(test.controller.steer("Change direction", [])).resolves.toBe(
      "rejected",
    );
    expect(test.states.at(-1)).toMatchObject({
      busy: false,
      error: { message: "The active Turn changed." },
    });
    expect(test.applied).toEqual([]);
  });

  it("reuses the queued message identity after a Steer acknowledgement timeout", async () => {
    const activeTurn = turn("turn-active", 1, "running");
    const activeSession = agentSessionFixture({
      ...session("hot", "1", null),
      capabilities: capabilitySetFixture({ activeTurnSteer: true }),
    });
    const test = harness(page("8", activeSession, [activeTurn]));
    vi.mocked(test.commands.steerTurn!)
      .mockRejectedValueOnce(
        new AcpDesktopError({
          kind: "conflict",
          resource: "steerOutcomeUnknown",
          message: "The message may have been delivered.",
        }),
      )
      .mockResolvedValueOnce({
        projection: page("9", activeSession, [activeTurn]),
      });

    await expect(
      test.controller.steer("Change direction", [], "queue-1" as never),
    ).resolves.toBe("outcomeUnknown");
    await expect(
      test.controller.steer("Change direction", [], "queue-1" as never),
    ).resolves.toBe("applied");
    expect(test.commands.steerTurn).toHaveBeenCalledTimes(2);
    expect(
      vi
        .mocked(test.commands.steerTurn!)
        .mock.calls.map(([request]) => request.clientIntentId),
    ).toEqual(["queue-1", "queue-1"]);
  });

  it("does not expose a queued Turn as cancellable", () => {
    const current = page("8", session("hot", "1", null), [
      turn("turn-queued", 2, "queued"),
    ]);

    expect(cancellableTurnId(current)).toBeNull();
  });

  it("restores a prompt only when typed failure proves it was not dispatched", async () => {
    const definite = harness(page("5", session("hot", "1", null)));
    vi.mocked(definite.commands.sendTurn).mockRejectedValue(
      new AcpDesktopError({
        kind: "conflict",
        resource: "projectionRevision",
        message: "stale",
      }),
    );
    await definite.controller.submit("restore me", [], bindingId, modelId);
    expect(definite.restored).toEqual([
      { prompt: "restore me", attachedContext: [] },
    ]);
    expect(definite.states.at(-1)?.error).toBeInstanceOf(AcpDesktopError);

    const preDispatchRecovery = harness(page("5", session("hot", "1", null)));
    vi.mocked(preDispatchRecovery.commands.sendTurn).mockRejectedValue(
      new AcpDesktopError({
        kind: "recoveryRequired",
        message: "snapshot integrity failed",
        promptDispatch: "notStarted",
      }),
    );
    await preDispatchRecovery.controller.submit(
      "restore validated context",
      [],
      bindingId,
      modelId,
    );
    expect(preDispatchRecovery.restored).toEqual([
      { prompt: "restore validated context", attachedContext: [] },
    ]);

    const uncertain = harness(page("5", session("hot", "1", null)));
    vi.mocked(uncertain.commands.sendTurn).mockRejectedValue(
      new AcpDesktopError({
        kind: "recoveryRequired",
        message: "delivery is unknown",
      }),
    );
    await uncertain.controller.submit("do not replay", [], bindingId, modelId);
    expect(uncertain.restored).toEqual([]);
    expect(uncertain.states.at(-1)?.error?.failure.kind).toBe(
      "recoveryRequired",
    );
  });

  it("never infers non-dispatch from the failure kind when native says unknown", async () => {
    const test = harness(page("5", session("hot", "1", null)));
    vi.mocked(test.commands.sendTurn).mockRejectedValue(
      new AcpDesktopError({
        kind: "invalidRequest",
        message: "Outcome is not proven",
        promptDispatch: "unknown",
      }),
    );
    await expect(
      test.controller.submit("do not replay", [], bindingId, modelId),
    ).resolves.toBe("rejected");
    expect(test.restored).toEqual([]);
  });

  it("closes only a hot current session with negotiated close capability", async () => {
    const test = harness(page("5", session("hot", "3", null)));
    vi.mocked(test.commands.closeSession).mockResolvedValue({
      projection: page("6", { ...session("hot", "3", null), state: "closing" }),
    });
    await expect(test.controller.close()).resolves.toBe("applied");
    expect(test.commands.closeSession).toHaveBeenCalledWith(
      expect.objectContaining({
        runtimeEpoch: "3",
        clientIntentId: "intent-1",
      }),
    );
  });

  it("ends an exact active session when protocol close is unavailable", async () => {
    const unavailable = page("5", {
      ...session("hot", "3", null),
      capabilities: { ...capabilities, cancel: true, closeSession: false },
    });
    const test = harness(unavailable);
    vi.mocked(test.commands.endSession).mockResolvedValue({
      projection: page("6", null),
    });

    await expect(test.controller.end()).resolves.toBe("applied");
    expect(test.commands.endSession).toHaveBeenCalledWith(
      expect.objectContaining({
        runtimeEpoch: "3",
        expectedProjectionRevision: "5",
        clientIntentId: "intent-1",
      }),
    );

    const characterized = harness(page("5", session("hot", "3", null)));
    await expect(characterized.controller.end()).resolves.toBe("rejected");
    expect(characterized.commands.endSession).not.toHaveBeenCalled();
  });

  it("creates bounded monotonic intent IDs with a stable per-controller prefix", () => {
    const next = createClientIntentFactory();
    const first = next();
    const second = next();
    expect(first).toMatch(/^intent-ui-0001-/);
    expect(second).toMatch(/^intent-ui-0002-/);
    expect(first.length).toBeLessThan(512);
    expect(second.length).toBeLessThan(512);
  });
});

it("stores a choice made on a parked session's restored config instead of sending it", async () => {
  const cold = {
    ...page("10", session("cold", null, "2")),
    restoredSessionConfig: {
      sageAgentSessionId: "session-a",
      modes: null,
      configOptions: [
        {
          id: "effort",
          name: "Effort",
          type: "select" as const,
          currentValue: "medium",
          options: [
            { value: "medium", name: "Medium" },
            { value: "high", name: "High" },
          ],
        },
      ],
    },
  };
  const test = harness(cold);
  vi.mocked(test.commands.queueSessionConfigChoice!).mockResolvedValue({
    projection: page("11", session("cold", null, "2")),
  });
  await expect(
    test.controller.setSessionConfigOption("effort", { value: "high" }),
  ).resolves.toBe("applied");
  expect(test.commands.queueSessionConfigChoice).toHaveBeenCalledWith({
    ...scope,
    projectGeneration: commandProjectGeneration,
    expectedProjectionRevision: "10",
    sessionId: "session-a",
    choice: { configId: "effort", value: { value: "high" } },
  });
  expect(test.commands.setSessionConfigOption).not.toHaveBeenCalled();
});

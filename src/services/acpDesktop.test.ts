import { acpHarnessDesktop } from "./acpHarnessDesktop";
import { afterEach, describe, expect, it, vi } from "vitest";

import type {
  ActivateAcpSessionRequest,
  CancelAcpTurnRequest,
  ClientIntentId,
  CloseAcpSessionRequest,
  CuratedBindingId,
  CodexAuthScope,
  EntityRevision,
  GetCodexDiscoveryRequest,
  ProjectGeneration,
  ProjectionRevision,
  RuntimeEpoch,
  SageAgentSessionId,
  SageChatId,
  SageProjectId,
  SageTurnId,
  SendAcpTurnRequest,
  StartAcpSessionRequest,
} from "../domain/acp";

const invokeMock = vi.hoisted(() => vi.fn());
const listenMock = vi.hoisted(() => vi.fn());

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
vi.mock("@tauri-apps/api/event", () => ({ listen: listenMock }));

import { AcpDesktopError, acpDesktop, safeFailure } from "./acpDesktop";
import { safePreviewProjectionChanged } from "./acpDesktopPreview";
import { acknowledgeReadTextFileFlush } from "./acpDesktopReadText";
import { safeTurn } from "./acpDesktopResponseTurns";

it("decodes persisted quote context only within the eight-entry UTF-8 bounds", () => {
  const quote = {
    kind: "transcriptQuote",
    ordinal: 1,
    sourceTurnId: "source-turn",
    sourceItemId: "source-item",
    text: "quoted",
    comment: "Why?",
    startOffset: 0,
    endOffset: 6,
  };
  const turn = {
    id: "turn",
    sessionId: "session",
    runtimeEpoch: "1",
    ordinal: 0,
    revision: "0",
    state: "completed",
    createdAt: 1,
    updatedAt: 2,
    terminalAt: 2,
    items: [],
    activities: [],
    attachedContext: [quote],
  };
  expect(safeTurn(turn)?.attachedContext).toEqual([quote]);
  expect(
    safeTurn({
      ...turn,
      attachedContext: Array.from({ length: 9 }, () => quote),
    }),
  ).toBeNull();
  expect(
    safeTurn({
      ...turn,
      attachedContext: [{ ...quote, text: "\u6c34".repeat(1366) }],
    }),
  ).toBeNull();
  expect(
    safeTurn({ ...turn, attachedContext: [{ ...quote, kind: "unknown" }] }),
  ).toBeNull();
});

const request: StartAcpSessionRequest = {
  projectId: "project-a" as SageProjectId,
  projectGeneration: "7" as ProjectGeneration,
  chatId: "chat-a" as SageChatId,
  expectedProjectionRevision: "9" as ProjectionRevision,
  clientIntentId: "intent-a" as ClientIntentId,
  bindingId: "binding-a" as CuratedBindingId,
  modelId: null,
};

const discoveryRequest: GetCodexDiscoveryRequest = {
  projectId: "project-a" as SageProjectId,
  projectGeneration: "7" as ProjectGeneration,
  bindingId: "codex-app-server" as CuratedBindingId,
};
const authRequest: CodexAuthScope = discoveryRequest;
const sessionRequest: ActivateAcpSessionRequest = {
  ...request,
  sessionId: "session-a" as SageAgentSessionId,
  runtimeEpoch: "2" as RuntimeEpoch,
};
const sendRequest: SendAcpTurnRequest = {
  ...sessionRequest,
  target: "session",
  prompt: "hello",
  attachedContext: [],
};
const cancelRequest: CancelAcpTurnRequest = {
  ...sessionRequest,
  turnId: "turn-a" as SageTurnId,
  expectedTurnState: "running",
  expectedTurnRevision: "1" as EntityRevision,
};

describe("safeFailure", () => {
  it("preserves the distinct Steer outcome unknown conflict", () => {
    expect(
      safeFailure({
        kind: "conflict",
        resource: "steerOutcomeUnknown",
        message: "The message may have been delivered.",
      }),
    ).toEqual({
      kind: "conflict",
      resource: "steerOutcomeUnknown",
      message: "The message may have been delivered.",
    });
  });
  it("preserves a nonfatal sign-in refusal from the native command", () => {
    expect(
      safeFailure({
        kind: "signInRequired",
        message: "The harness requires sign-in before it can continue.",
        promptDispatch: "unknown",
      }),
    ).toEqual({
      kind: "signInRequired",
      message: "The harness requires sign-in before it can continue.",
      promptDispatch: "unknown",
    });
  });
});
const closeRequest: CloseAcpSessionRequest = sessionRequest;

function setDesktopRuntime(active: boolean): void {
  if (active) {
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      value: {},
    });
  } else {
    Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  }
}

afterEach(() => {
  setDesktopRuntime(false);
  invokeMock.mockReset();
  listenMock.mockReset();
});

describe("ACP desktop bridge", () => {
  it("sends bounded native Goal pause and clear requests and keeps the authoritative Goal projection", async () => {
    setDesktopRuntime(true);
    const projection = {
      projectId: "project-a",
      chatId: "chat-a",
      title: "A",
      projectionRevision: "10",
      currentSessionId: null,
      sessions: [],
      turns: [],
      nextCursor: null,
      goal: {
        kind: "goal",
        snapshot: {
          objective: "Ship safely",
          status: "paused",
          tokenBudget: null,
          tokensUsed: 4,
          timeUsedSeconds: 5,
          createdAt: null,
          updatedAt: null,
        },
      },
    };
    invokeMock.mockResolvedValueOnce({ projection });

    const paused = await acpDesktop.pauseExistingGoal(sessionRequest);

    expect(invokeMock).toHaveBeenCalledWith("pause_existing_goal", {
      request: sessionRequest,
    });
    expect(paused.projection.goal).toEqual(projection.goal);

    invokeMock.mockResolvedValueOnce({
      projection: { ...projection, goal: { kind: "noGoal" } },
    });
    const cleared = await acpDesktop.clearExistingGoal(sessionRequest);
    expect(invokeMock).toHaveBeenLastCalledWith("clear_existing_goal", {
      request: sessionRequest,
    });
    expect(cleared.projection.goal).toEqual({ kind: "noGoal" });
  });

  it("sends exact owner and independent config revision for typed config mutations", async () => {
    setDesktopRuntime(true);
    invokeMock.mockResolvedValue({
      projection: {
        projectId: "project-a",
        chatId: "chat-a",
        title: "A",
        projectionRevision: "9",
        currentSessionId: null,
        sessions: [],
        turns: [],
        nextCursor: null,
        sessionConfig: null,
      },
    });
    const owner = {
      projectId: request.projectId,
      projectGeneration: request.projectGeneration,
      chatId: request.chatId,
      expectedProjectionRevision: request.expectedProjectionRevision,
      clientIntentId: request.clientIntentId,
      sessionId: sessionRequest.sessionId,
      runtimeEpoch: sessionRequest.runtimeEpoch,
      expectedConfigRevision: "3",
    };
    await acpDesktop.setSessionMode({ ...owner, modeId: "ask" });
    expect(invokeMock).toHaveBeenLastCalledWith("set_acp_session_mode", {
      request: { ...owner, modeId: "ask" },
    });
    await acpDesktop.setSessionConfigOption({
      ...owner,
      configId: "flag",
      value: { type: "boolean", value: false },
    });
    expect(invokeMock).toHaveBeenLastCalledWith(
      "set_acp_session_config_option",
      {
        request: {
          ...owner,
          configId: "flag",
          value: { type: "boolean", value: false },
        },
      },
    );
  });
  it("subscribes to the separate config wake channel with strict decoding", async () => {
    setDesktopRuntime(true);
    const onChange = vi.fn();
    listenMock.mockResolvedValue(() => undefined);
    await acpDesktop.subscribeSessionConfigChanges(onChange);
    expect(listenMock.mock.calls[0][0]).toBe("acp-session-config-changed-v1");
    const callback = listenMock.mock.calls[0][1];
    const event = {
      schemaVersion: 1,
      sageProjectId: "project-a",
      sageChatId: "chat-a",
      sageAgentSessionId: "session-a",
      runtimeEpoch: "2",
      configRevision: "3",
    };
    callback({ payload: { ...event, configRevision: "0" } });
    expect(onChange).not.toHaveBeenCalled();
    callback({ payload: { ...event, raw: "discard" } });
    expect(onChange).toHaveBeenCalledWith(event);
  });
  it("projects only sanitized auth lifecycle fields and sends explicit consent", async () => {
    setDesktopRuntime(true);
    invokeMock.mockResolvedValue({
      ...authRequest,
      phase: "signedIn",
      revision: "3",
      authUrl: "https://auth.openai.com/secret",
      loginId: "never-forward",
      email: "never-forward@example.test",
    });

    const response = await acpHarnessDesktop.startCodexAuth(authRequest);

    expect(invokeMock).toHaveBeenCalledWith("start_codex_auth", {
      request: { ...authRequest, consent: true },
    });
    expect(response).toEqual({
      ...authRequest,
      phase: "signedIn",
      revision: "3",
    });
    expect(JSON.stringify(response)).not.toContain("never-forward");
  });

  it("projects only Claude's boolean sign-in fact", async () => {
    invokeMock.mockResolvedValue({ loggedIn: true, token: "never-forward" });
    expect(await acpDesktop.getClaudeAuthStatus()).toEqual({ loggedIn: true });
    expect(invokeMock).toHaveBeenCalledWith(
      "get_claude_auth_status",
      undefined,
    );
    invokeMock.mockResolvedValue({ loggedIn: "true" });
    await expect(acpDesktop.getClaudeAuthStatus()).rejects.toThrow(
      "Claude Code sign-in status was invalid.",
    );
  });

  it("rejects invalid auth phases and revisions at the WebView boundary", async () => {
    setDesktopRuntime(true);
    invokeMock.mockResolvedValue({
      ...authRequest,
      phase: "consent",
      revision: "0",
    });
    await expect(
      acpHarnessDesktop.cancelCodexAuth(authRequest),
    ).rejects.toMatchObject({
      failure: { kind: "internal" },
    });
  });

  it("accepts only the sanitized auth recovery projection", async () => {
    setDesktopRuntime(true);
    invokeMock.mockResolvedValue({
      ...authRequest,
      phase: "recovery",
      revision: "4",
      error: "never-forward",
    });

    await expect(
      acpHarnessDesktop.cancelCodexAuth(authRequest),
    ).resolves.toEqual({
      ...authRequest,
      phase: "recovery",
      revision: "4",
    });
  });
  it("validates and narrows the bounded Codex discovery response", async () => {
    setDesktopRuntime(true);
    invokeMock.mockResolvedValue({
      projectId: "project-a",
      projectGeneration: "7",
      rootId: "root-a",
      bindingId: "codex-app-server",
      runtimeEpoch: "2",
      account: "managedChatgpt",
      models: [
        { id: "gpt-a", name: "GPT A", isDefault: false },
        { id: "gpt-b", name: "GPT B", isDefault: true },
      ],
      defaultModelId: "gpt-b",
      email: "never-retain@example.test",
      rawFrame: "never-retain",
    });

    const response = await acpHarnessDesktop.discoverCodex(discoveryRequest);

    expect(invokeMock).toHaveBeenCalledWith("get_codex_discovery", {
      request: discoveryRequest,
    });
    expect(response.defaultModelId).toBe("gpt-b");
    expect(JSON.stringify(response)).not.toContain("never-retain");
  });

  it("rejects duplicate or inconsistent discovery models", async () => {
    setDesktopRuntime(true);
    invokeMock.mockResolvedValue({
      projectId: "project-a",
      projectGeneration: "7",
      rootId: "root-a",
      bindingId: "codex-app-server",
      runtimeEpoch: "2",
      account: "managedChatgpt",
      models: [
        { id: "same", name: "One", isDefault: true },
        { id: "same", name: "Two", isDefault: false },
      ],
      defaultModelId: "same",
    });

    await expect(
      acpHarnessDesktop.discoverCodex(discoveryRequest),
    ).rejects.toMatchObject({
      failure: { kind: "internal" },
    });
  });

  it("accepts only an empty signed-out discovery projection", async () => {
    setDesktopRuntime(true);
    invokeMock.mockResolvedValue({
      projectId: "project-a",
      projectGeneration: "7",
      rootId: "root-a",
      bindingId: "codex-app-server",
      runtimeEpoch: "2",
      account: "signedOut",
      models: [],
      defaultModelId: null,
      email: "never-retain@example.test",
    });

    const response = await acpHarnessDesktop.discoverCodex(discoveryRequest);
    expect(response).toEqual({
      projectId: "project-a",
      projectGeneration: "7",
      rootId: "root-a",
      bindingId: "codex-app-server",
      runtimeEpoch: "2",
      account: "signedOut",
      models: [],
      defaultModelId: null,
    });
  });

  it("accepts a provider-managed discovery projection", async () => {
    setDesktopRuntime(true);
    invokeMock.mockResolvedValue({
      projectId: "project-a",
      projectGeneration: "7",
      rootId: "root-a",
      bindingId: "codex-app-server",
      runtimeEpoch: "2",
      account: "providerManaged",
      models: [
        { id: "provider-model", name: "Provider model", isDefault: true },
      ],
      defaultModelId: "provider-model",
    });

    await expect(
      acpHarnessDesktop.discoverCodex(discoveryRequest),
    ).resolves.toMatchObject({
      account: "providerManaged",
      defaultModelId: "provider-model",
    });
  });

  it("rejects discovery counters beyond the native signed range", async () => {
    setDesktopRuntime(true);
    invokeMock.mockResolvedValue({
      projectId: "project-a",
      projectGeneration: "9223372036854775808",
      rootId: "root-a",
      bindingId: "codex-app-server",
      runtimeEpoch: "2",
      account: "managedChatgpt",
      models: [{ id: "gpt-a", name: "GPT A", isDefault: true }],
      defaultModelId: "gpt-a",
    });

    await expect(
      acpHarnessDesktop.discoverCodex(discoveryRequest),
    ).rejects.toMatchObject({
      failure: { kind: "internal" },
    });
  });

  it("rejects NUL-bearing discovery identifiers at the WebView boundary", async () => {
    setDesktopRuntime(true);
    invokeMock.mockResolvedValue({
      projectId: "project-a",
      projectGeneration: "7",
      rootId: "root-a",
      bindingId: "codex-app-server",
      runtimeEpoch: "2",
      account: "managedChatgpt",
      models: [{ id: "gpt\0a", name: "GPT A", isDefault: true }],
      defaultModelId: "gpt\0a",
    });

    await expect(
      acpHarnessDesktop.discoverCodex(discoveryRequest),
    ).rejects.toMatchObject({
      failure: { kind: "internal" },
    });
  });

  it("nests the exact closed request under the Tauri request argument", async () => {
    setDesktopRuntime(true);
    invokeMock.mockRejectedValue({
      kind: "unavailable",
      reason: "desktopRuntimeUnavailable",
      message: "Runtime is not active.",
    });

    await expect(acpDesktop.startSession(request)).rejects.toBeInstanceOf(
      AcpDesktopError,
    );
    expect(invokeMock).toHaveBeenCalledWith("start_acp_session", { request });
  });

  it.each([
    { kind: "recoveryRequired", promptDispatch: "notStarted" },
    { kind: "internal", promptDispatch: "notStarted" },
    { kind: "recoveryRequired", promptDispatch: "unknown" },
    { kind: "internal", promptDispatch: "unknown" },
  ] as const)(
    "preserves native $kind dispatch status $promptDispatch",
    async ({ kind, promptDispatch }) => {
      setDesktopRuntime(true);
      const failure = { kind, message: "Native failure" };
      for (const rejection of [
        { ...failure, promptDispatch },
        { failure, promptDispatch },
      ]) {
        invokeMock.mockRejectedValue(rejection);
        await expect(acpDesktop.sendTurn(sendRequest)).rejects.toMatchObject({
          failure: { kind, promptDispatch },
        });
      }
    },
  );

  it("preserves proof that a send failed before prompt dispatch", async () => {
    setDesktopRuntime(true);
    invokeMock.mockRejectedValue({
      failure: {
        kind: "recoveryRequired",
        message: "Attached context failed its integrity check.",
      },
      promptDispatch: "notStarted",
    });

    await expect(acpDesktop.sendTurn(sendRequest)).rejects.toMatchObject({
      failure: {
        kind: "recoveryRequired",
        promptDispatch: "notStarted",
      },
    });

    invokeMock.mockRejectedValue({
      failure: {
        kind: "recoveryRequired",
        message: "Delivery is unknown.",
      },
      promptDispatch: "unknown",
    });
    await expect(acpDesktop.sendTurn(sendRequest)).rejects.toMatchObject({
      failure: { kind: "recoveryRequired" },
    });
    await expect(acpDesktop.sendTurn(sendRequest)).rejects.not.toMatchObject({
      failure: { promptDispatch: "notStarted" },
    });
  });

  it("preserves the typed image-input block without consuming a prompt", async () => {
    setDesktopRuntime(true);
    invokeMock.mockRejectedValue({
      failure: {
        kind: "unavailable",
        reason: "imageInputUnsupported",
        message: "This harness cannot accept image attachments yet.",
      },
      promptDispatch: "notStarted",
    });
    await expect(acpDesktop.sendTurn(sendRequest)).rejects.toMatchObject({
      failure: {
        kind: "unavailable",
        reason: "imageInputUnsupported",
        promptDispatch: "notStarted",
      },
    });
  });

  it.each([
    {
      name: "start",
      command: "start_acp_session",
      invoke: () => acpDesktop.startSession(request),
    },
    {
      name: "activate",
      command: "activate_acp_session",
      invoke: () => acpDesktop.activateSession(sessionRequest),
    },
    {
      name: "send",
      command: "send_acp_turn",
      invoke: () => acpDesktop.sendTurn(sendRequest),
    },
    {
      name: "cancel",
      command: "cancel_acp_turn",
      invoke: () => acpDesktop.cancelTurn(cancelRequest),
    },
    {
      name: "close",
      command: "close_acp_session",
      invoke: () => acpDesktop.closeSession(closeRequest),
    },
  ])(
    "rejects an invalid $name mutation projection",
    async ({ command, invoke }) => {
      setDesktopRuntime(true);
      invokeMock.mockResolvedValue({
        projection: { projectId: "project-a", rawFrame: "never-retain" },
      });

      await expect(invoke()).rejects.toMatchObject({
        failure: {
          kind: "internal",
          message: "ACP mutation returned an invalid bounded projection.",
        },
      });
      expect(invokeMock).toHaveBeenCalledWith(command, {
        request: expect.any(Object),
      });
    },
  );

  it("retains only a bounded mutation projection", async () => {
    setDesktopRuntime(true);
    invokeMock.mockResolvedValue({
      projection: {
        projectId: "project-a",
        chatId: "chat-a",
        title: "A",
        projectionRevision: "10",
        currentSessionId: null,
        sessions: [],
        turns: [],
        nextCursor: null,
        rawFrame: "never-retain",
      },
      processSecret: "never-retain",
    });

    await expect(acpDesktop.startSession(request)).resolves.toEqual({
      projection: {
        sessionConfig: null,
        projectId: "project-a",
        chatId: "chat-a",
        title: "A",
        projectionRevision: "10",
        currentSessionId: null,
        sessions: [],
        turns: [],
        nextCursor: null,
      },
    });
  });

  it("retains only the typed safe failure and drops unknown native fields", async () => {
    setDesktopRuntime(true);
    invokeMock.mockRejectedValue({
      kind: "conflict",
      resource: "projectGeneration",
      message: "The project generation is stale.",
      rawFrame: "secret-never-retain",
    });

    const error = await acpDesktop
      .startSession(request)
      .catch((value) => value);
    expect(error).toBeInstanceOf(AcpDesktopError);
    expect((error as AcpDesktopError).failure).toEqual({
      kind: "conflict",
      resource: "projectGeneration",
      message: "The project generation is stale.",
    });
    expect(JSON.stringify(error)).not.toContain("secret-never-retain");
  });

  it("preserves a bounded typed harness rejection without retaining native fields", async () => {
    setDesktopRuntime(true);
    invokeMock.mockRejectedValue({
      kind: "rejected",
      message: "The harness rejected this session configuration change.",
      remoteMessage: "secret-never-retain",
    });

    const error = await acpDesktop
      .startSession(request)
      .catch((value) => value);
    expect(error).toBeInstanceOf(AcpDesktopError);
    expect((error as AcpDesktopError).failure).toEqual({
      kind: "rejected",
      message: "The harness rejected this session configuration change.",
    });
    expect(JSON.stringify(error)).not.toContain("secret-never-retain");
  });

  it("fails closed without importing or invoking the native bridge in a browser", async () => {
    setDesktopRuntime(false);
    await expect(acpDesktop.startSession(request)).rejects.toMatchObject({
      failure: {
        kind: "unavailable",
        reason: "desktopRuntimeUnavailable",
      },
    });
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("forwards only bounded canonical projection notices", async () => {
    setDesktopRuntime(true);
    const unlisten = vi.fn();
    let receive: ((event: { payload: unknown }) => void) | null = null;
    listenMock.mockImplementation(
      async (
        _name: string,
        callback: (event: { payload: unknown }) => void,
      ) => {
        receive = callback;
        return unlisten;
      },
    );
    const onChange = vi.fn();

    const dispose = await acpDesktop.subscribeProjectionChanges(onChange);
    expect(listenMock).toHaveBeenCalledWith(
      "acp-projection-changed-v1",
      expect.any(Function),
    );
    const send = receive as unknown as (event: { payload: unknown }) => void;
    send({
      payload: {
        projectId: "project-a",
        chatId: "chat-a",
        sessionId: "session-a",
        projectionRevision: "10",
      },
    });
    send({
      payload: {
        projectId: "project-a",
        chatId: "chat-a",
        sessionId: "session-a",
        projectionRevision: "0",
        rawFrame: "never-forward",
      },
    });

    expect(onChange).toHaveBeenCalledTimes(1);
    expect(onChange).toHaveBeenCalledWith({
      projectId: "project-a",
      chatId: "chat-a",
      sessionId: "session-a",
      projectionRevision: "10",
    });
    dispose();
    expect(unlisten).toHaveBeenCalledOnce();
  });

  it("routes only the exact native Preview projection and leaves ACP notices compatible", async () => {
    setDesktopRuntime(true);
    const unlisten = vi.fn();
    let receive: ((event: { payload: unknown }) => void) | null = null;
    listenMock.mockImplementation(async (_name, callback) => {
      receive = callback;
      return unlisten;
    });
    const onPreview = vi.fn();
    const dispose =
      await acpDesktop.subscribePreviewProjectionChanges(onPreview);
    const send = receive as unknown as (event: { payload: unknown }) => void;
    const preview = {
      projectId: "project-a",
      projectGeneration: "7",
      transitionEpoch: 7,
      chatId: "chat-a",
      sessionId: "session-a",
      runtimeEpoch: "3",
      previewRevision: "9223372036854775807",
      phase: "idle",
      failure: null,
      target: null,
      origin: null,
    };
    send({ payload: { kind: "preview", preview } });
    send({
      payload: {
        kind: "preview",
        preview: { ...preview, transitionEpoch: "7" },
      },
    });
    send({
      payload: {
        kind: "preview",
        preview: { ...preview, projectGeneration: "8" },
      },
    });
    send({
      payload: {
        kind: "preview",
        preview: {
          ...preview,
          target: {
            projectId: "project-a",
            projectGeneration: "8",
            expectedTransitionEpoch: 7,
            chatId: "chat-a",
            sessionId: "session-a",
            runtimeEpoch: "3",
            rootId: "root-a",
            scriptName: "dev",
          },
        },
      },
    });
    send({
      payload: {
        kind: "preview",
        preview: { ...preview, phase: "idle", failure: "healthTimeout" },
      },
    });
    send({
      payload: {
        projectId: "project-a",
        chatId: "chat-a",
        sessionId: "session-a",
        projectionRevision: "2",
      },
    });
    expect(onPreview).toHaveBeenCalledTimes(1);
    expect(onPreview).toHaveBeenCalledWith({ kind: "preview", preview });
    dispose();
    expect(unlisten).toHaveBeenCalledOnce();
  });

  it("admits an unsupported https origin failure and still rejects unknown failures", () => {
    const failed = {
      projectId: "project-a",
      projectGeneration: "7",
      transitionEpoch: 7,
      chatId: "chat-a",
      sessionId: "session-a",
      runtimeEpoch: "3",
      previewRevision: "4",
      phase: "healthFailed",
      failure: "originUnsupported",
      target: {
        projectId: "project-a",
        projectGeneration: "7",
        expectedTransitionEpoch: 7,
        chatId: "chat-a",
        sessionId: "session-a",
        runtimeEpoch: "3",
        rootId: "root-a",
        scriptName: "dev",
      },
      origin: null,
    };

    expect(
      safePreviewProjectionChanged({ kind: "preview", preview: failed }),
    ).toEqual({ kind: "preview", preview: failed });
    expect(
      safePreviewProjectionChanged({
        kind: "preview",
        preview: { ...failed, failure: "tlsUnsupported" },
      }),
    ).toBeNull();
    expect(
      safePreviewProjectionChanged({
        kind: "preview",
        preview: { ...failed, phase: "exited" },
      }),
    ).toBeNull();
  });

  it("invokes the exact native P-02 acknowledgement envelope", async () => {
    invokeMock.mockResolvedValue(undefined);
    const ack = {
      owner: {
        projectId: "project-a",
        projectGeneration: "7",
        chatId: "chat-a",
        sessionId: "session-a",
        runtimeEpoch: "2",
      },
      resource: { rootId: "root-a", relativePath: "file.txt" },
      expectedEditorRevision: null,
      token: "p02-1",
      state: {
        kind: "diskAllowed" as const,
        state: "notOpen" as const,
        editorRevision: null,
      },
    };
    await acknowledgeReadTextFileFlush(ack);
    expect(invokeMock).toHaveBeenCalledWith("acknowledge_acp_read_text_file", {
      ack,
    });
  });
});

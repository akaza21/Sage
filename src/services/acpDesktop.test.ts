import { afterEach, describe, expect, it, vi } from "vitest";

import type {
  ClientIntentId,
  CuratedBindingId,
  GetCodexDiscoveryRequest,
  ProjectGeneration,
  ProjectionRevision,
  SageChatId,
  SageProjectId,
  StartAcpSessionRequest,
} from "../domain/acp";

const invokeMock = vi.hoisted(() => vi.fn());
const listenMock = vi.hoisted(() => vi.fn());

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
vi.mock("@tauri-apps/api/event", () => ({ listen: listenMock }));

import { AcpDesktopError, acpDesktop } from "./acpDesktop";

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

    const response = await acpDesktop.discoverCodex(discoveryRequest);

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
      acpDesktop.discoverCodex(discoveryRequest),
    ).rejects.toMatchObject({
      failure: { kind: "internal" },
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
      acpDesktop.discoverCodex(discoveryRequest),
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
      acpDesktop.discoverCodex(discoveryRequest),
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
});

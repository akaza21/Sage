import { afterEach, expect, it, vi } from "vitest";
import type { GetAcpChatPageRequest } from "../domain/acp";
import { mergeProjectionPage } from "../app/acpProjectionMerge";
import { acpDesktop } from "./acpDesktop";
import { presentedAttentionPosition } from "./chatAttention";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

afterEach(() => {
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  invoke.mockReset();
});

const request = {
  projectId: "project-1",
  projectGeneration: "1",
  chatId: "chat-1",
  cursor: null,
} as GetAcpChatPageRequest;

function nativePage(position: unknown, chunk = 1) {
  return {
    projectId: request.projectId,
    chatId: request.chatId,
    title: "Streaming chat",
    projectionRevision: "2",
    currentSessionId: "session-1",
    sessions: [
      {
        id: "session-1",
        ordinal: 0,
        bindingId: "fixture",
        modelId: null,
        state: "active",
        residency: "hot",
        runtimeEpoch: "1",
        nextRuntimeEpoch: null,
        capabilities: {
          schemaVersion: 1,
          protocolVersion: "fixture-1",
          newSession: true,
          resumeSession: false,
          loadSession: false,
          closeSession: false,
          prompt: true,
          cancel: true,
          nativeSessionFork: false,
          activeTurnSteer: false,
          childConversationProjection: false,
          childTranscriptRead: false,
          collaborationEventProjection: false,
          multiRootExtensionSchemaId: null,
          modelCount: 0,
          configCount: 0,
        },
        permissionRequests: false,
        revision: "1",
        projectionRevision: "2",
        legacy: false,
      },
    ],
    turns: [
      {
        id: "turn-1",
        sessionId: "session-1",
        runtimeEpoch: "1",
        ordinal: 0,
        state: "running",
        revision: String(chunk),
        createdAt: 1,
        updatedAt: chunk + 1,
        terminalAt: null,
        items: [
          {
            id: "item-1",
            projectId: request.projectId,
            chatId: request.chatId,
            sessionId: "session-1",
            turnId: "turn-1",
            runtimeEpoch: "1",
            activityId: null,
            ordinal: 0,
            origin: "primary",
            kind: "text",
            lifecycle: "streaming",
            toolState: null,
            revision: String(chunk),
            createdAt: 1,
            updatedAt: chunk + 1,
            terminalAt: null,
            payload: { role: "assistant", text: `chunk-${chunk}` },
          },
        ],
        activities: [],
      },
    ],
    nextCursor: null,
    observedAttentionPosition: position,
  };
}

it("streamed transcript refreshes make zero extra attention-query IPC calls", async () => {
  Object.defineProperty(window, "__TAURI_INTERNALS__", {
    configurable: true,
    value: {},
  });
  let chunk = 0;
  invoke.mockImplementation(async (command: string) =>
    command === "get_chat_attention"
      ? [{ activityPosition: "99" }]
      : nativePage("7", ++chunk),
  );

  const positions: (string | undefined)[] = [];
  for (let refresh = 0; refresh < 100; refresh += 1) {
    const page = await acpDesktop.getChatPage(request);
    positions.push(presentedAttentionPosition(page));
    expect(page.turns[0]).toMatchObject({
      state: "running",
      items: [{ payload: { text: `chunk-${refresh + 1}` } }],
    });
  }

  expect(
    invoke.mock.calls.filter(([command]) => command === "get_chat_attention"),
  ).toHaveLength(0);
  expect(new Set(positions)).toEqual(new Set(["7"]));
  expect(invoke).toHaveBeenCalledTimes(100);
  expect(invoke).toHaveBeenLastCalledWith("get_acp_chat_page", { request });

  // Earlier history cannot acknowledge activity newer than the presented latest page.
  const current = await acpDesktop.getChatPage(request);
  invoke.mockResolvedValue(nativePage("8"));
  const earlier = await acpDesktop.getChatPage({
    ...request,
    cursor: "older",
  } as GetAcpChatPageRequest);
  expect(presentedAttentionPosition(earlier)).toBeUndefined();
  expect(
    presentedAttentionPosition(mergeProjectionPage(current, earlier)),
  ).toBe("7");
});

it.each(["0", undefined, null, "-1", "9223372036854775808"])(
  "only acknowledges a valid snapshot position: %s",
  async (position) => {
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      value: {},
    });
    invoke.mockImplementation(async (command: string) =>
      command === "get_chat_attention" ? [] : nativePage(position),
    );
    const page = await acpDesktop.getChatPage(request);
    expect(presentedAttentionPosition(page)).toBe(
      position === "0" ? "0" : undefined,
    );
  },
);

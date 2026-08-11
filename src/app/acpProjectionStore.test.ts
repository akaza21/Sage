import { beforeEach, describe, expect, it } from "vitest";

import type {
  AcpChatPage,
  AcpItem,
  AcpPageCursor,
  AcpProjectionChangedEvent,
  AgentSessionProjection,
  EntityRevision,
  ProjectGeneration,
  ProjectionRevision,
  RuntimeEpoch,
  SageAgentSessionId,
  SageChatId,
  SageItemId,
  SageProjectId,
  SageTurnId,
  TurnProjection,
} from "../domain/acp";
import {
  MAX_RETAINED_BYTES,
  MAX_RETAINED_ITEMS,
  MAX_RETAINED_TURNS,
  useAcpProjectionStore,
} from "./acpProjectionStore";

const projectId = "project-a" as SageProjectId;
const chatId = "chat-a" as SageChatId;
const sessionId = "session-a" as SageAgentSessionId;
const generation = "7" as ProjectGeneration;
const scope = { projectId, projectGeneration: generation, chatId };

const capabilities: AgentSessionProjection["capabilities"] = {
  schemaVersion: 1,
  protocolVersion: "fixture-v1",
  newSession: true,
  resumeSession: false,
  loadSession: false,
  closeSession: true,
  prompt: true,
  cancel: true,
  nativeSessionFork: false,
  activeTurnSteer: false,
  childConversationProjection: false,
  childTranscriptRead: false,
  collaborationEventProjection: false,
  multiRootExtensionSchemaId: null,
  modelCount: 1,
  configCount: 0,
};

function item(turn: number, ordinal: number, text = "bounded"): AcpItem {
  return {
    id: `item-${turn}-${ordinal}` as SageItemId,
    projectId,
    chatId,
    sessionId,
    turnId: `turn-${turn}` as SageTurnId,
    runtimeEpoch: "1" as RuntimeEpoch,
    ordinal,
    origin: "primary",
    activityId: null,
    lifecycle: "completed",
    revision: "1" as EntityRevision,
    createdAt: ordinal,
    updatedAt: ordinal,
    terminalAt: ordinal,
    kind: "text",
    toolState: null,
    payload: { role: "assistant", text },
  };
}

function turn(ordinal: number, items = [item(ordinal, 0)]): TurnProjection {
  return {
    id: `turn-${ordinal}` as SageTurnId,
    sessionId,
    runtimeEpoch: "1" as RuntimeEpoch,
    ordinal,
    state: "completed",
    revision: "1" as EntityRevision,
    createdAt: ordinal,
    updatedAt: ordinal,
    terminalAt: ordinal,
    items,
    activities: [],
  };
}

function page(revision: string, turns: TurnProjection[]): AcpChatPage {
  return {
    projectId,
    chatId,
    title: "Projection fixture",
    projectionRevision: revision as ProjectionRevision,
    currentSessionId: sessionId,
    sessions: [
      {
        id: sessionId,
        ordinal: 0,
        bindingId: "binding-a" as AgentSessionProjection["bindingId"],
        modelId: null,
        state: "closed",
        residency: "cold",
        runtimeEpoch: null,
        nextRuntimeEpoch: null,
        capabilities,
        revision: "1" as EntityRevision,
        projectionRevision: revision as ProjectionRevision,
        legacy: false,
      },
    ],
    turns,
    nextCursor: null,
  };
}

beforeEach(() => {
  useAcpProjectionStore.getState().clear();
  useAcpProjectionStore.getState().activateScope(scope);
});

describe("ACP projection request intents", () => {
  it("ignores a deferred completion superseded by a newer request", async () => {
    const oldIntent = useAcpProjectionStore.getState().beginPageLoad(null);
    let resolveOld: ((value: AcpChatPage) => void) | undefined;
    const oldCompletion = new Promise<AcpChatPage>((resolve) => {
      resolveOld = resolve;
    });
    const currentIntent = useAcpProjectionStore.getState().beginPageLoad(null);
    resolveOld?.(page("1", [turn(1)]));

    const oldResult = useAcpProjectionStore
      .getState()
      .completePageLoad(oldIntent, await oldCompletion);
    expect(oldResult).toBe("ignoredStaleIntent");
    expect(useAcpProjectionStore.getState().page).toBeNull();

    expect(
      useAcpProjectionStore
        .getState()
        .completePageLoad(currentIntent, page("2", [turn(2)])),
    ).toBe("accepted");
    expect(useAcpProjectionStore.getState().page?.turns[0].ordinal).toBe(2);
  });

  it("rejects stale and noncanonical projection revisions", () => {
    expect(
      useAcpProjectionStore
        .getState()
        .applyAuthoritativePage(scope, page("10", [])),
    ).toBe("accepted");
    expect(
      useAcpProjectionStore
        .getState()
        .applyAuthoritativePage(scope, page("9", [])),
    ).toBe("ignoredStaleRevision");
    expect(
      useAcpProjectionStore
        .getState()
        .applyAuthoritativePage(scope, page("010", [])),
    ).toBe("ignoredInvalidRevision");
  });

  it("rejects an old mutation completion after same-chat reactivation", () => {
    useAcpProjectionStore.getState().activateScope({
      ...scope,
      projectGeneration: "8" as ProjectGeneration,
    });
    expect(
      useAcpProjectionStore
        .getState()
        .applyAuthoritativePage(scope, page("20", [turn(20)])),
    ).toBe("ignoredStaleIntent");
    expect(useAcpProjectionStore.getState().page).toBeNull();
  });

  it("ends a matching load whose revision is rejected", () => {
    expect(
      useAcpProjectionStore
        .getState()
        .applyAuthoritativePage(scope, page("10", [])),
    ).toBe("accepted");
    const intent = useAcpProjectionStore.getState().beginPageLoad(null);
    expect(
      useAcpProjectionStore.getState().completePageLoad(intent, page("9", [])),
    ).toBe("ignoredStaleRevision");
    expect(useAcpProjectionStore.getState().activeLoad).toBeNull();
    expect(useAcpProjectionStore.getState().error).toBeNull();
  });
});

describe("ACP projection bounds and notification mailbox", () => {
  it("merges same-Turn history fragments without dropping or duplicating Items", () => {
    const newest = page("1", [
      turn(
        0,
        Array.from({ length: 256 }, (_, index) => item(0, index + 256)),
      ),
    ]);
    newest.nextCursor = "same-turn-cursor" as AcpPageCursor;
    const latestIntent = useAcpProjectionStore.getState().beginPageLoad(null);
    expect(
      useAcpProjectionStore.getState().completePageLoad(latestIntent, newest),
    ).toBe("accepted");
    expect(useAcpProjectionStore.getState().retentionIncomplete).toBe(false);
    expect(useAcpProjectionStore.getState().page?.nextCursor).toBe(
      "same-turn-cursor",
    );

    const historyIntent = useAcpProjectionStore
      .getState()
      .beginPageLoad(newest.nextCursor);
    const older = page("1", [
      turn(
        0,
        Array.from({ length: 256 }, (_, index) => item(0, index)),
      ),
    ]);
    expect(
      useAcpProjectionStore.getState().completePageLoad(historyIntent, older),
    ).toBe("accepted");

    const merged = useAcpProjectionStore.getState().page?.turns[0];
    expect(merged?.items).toHaveLength(512);
    expect(merged?.items[0].ordinal).toBe(0);
    expect(merged?.items[511].ordinal).toBe(511);
    expect(new Set(merged?.items.map((value) => value.id)).size).toBe(512);
    expect(useAcpProjectionStore.getState().page?.nextCursor).toBeNull();
    expect(useAcpProjectionStore.getState().retentionIncomplete).toBe(false);
  });

  it("retains at most 100 Turns, 1,000 Items and 8 MiB", () => {
    const turns = Array.from({ length: 110 }, (_, turnIndex) =>
      turn(
        turnIndex,
        Array.from({ length: 11 }, (_, itemIndex) =>
          item(turnIndex, itemIndex, "x".repeat(9_000)),
        ),
      ),
    );
    expect(
      useAcpProjectionStore
        .getState()
        .applyAuthoritativePage(scope, page("1", turns)),
    ).toBe("accepted");

    const state = useAcpProjectionStore.getState();
    const retainedItems =
      state.page?.turns.reduce(
        (total, value) => total + value.items.length,
        0,
      ) ?? 0;
    expect(state.page?.turns.length).toBeLessThanOrEqual(MAX_RETAINED_TURNS);
    expect(retainedItems).toBeLessThanOrEqual(MAX_RETAINED_ITEMS);
    expect(state.retainedBytes).toBeLessThanOrEqual(MAX_RETAINED_BYTES);
    expect(state.page?.turns.at(-1)?.ordinal).toBe(109);
    expect(state.retentionIncomplete).toBe(true);

    expect(state.applyAuthoritativePage(scope, page("2", [turn(110)]))).toBe(
      "accepted",
    );
    expect(useAcpProjectionStore.getState().retentionIncomplete).toBe(true);
  });

  it("does not carry retention loss into a different Chat scope", () => {
    useAcpProjectionStore.setState({ retentionIncomplete: true });

    useAcpProjectionStore.getState().activateScope({
      ...scope,
      chatId: "chat-b" as SageChatId,
    });

    expect(useAcpProjectionStore.getState().retentionIncomplete).toBe(false);
  });

  it("coalesces only the same notification tuple into one slot", () => {
    const notice = (revision: string): AcpProjectionChangedEvent => ({
      projectId,
      chatId,
      sessionId,
      projectionRevision: revision as ProjectionRevision,
    });
    expect(
      useAcpProjectionStore.getState().enqueueProjectionChange(notice("7")),
    ).toBe("queued");
    expect(
      useAcpProjectionStore.getState().enqueueProjectionChange(notice("9")),
    ).toBe("coalesced");
    expect(
      useAcpProjectionStore.getState().enqueueProjectionChange(notice("8")),
    ).toBe("ignoredStale");
    expect(
      useAcpProjectionStore.getState().enqueueProjectionChange({
        ...notice("10"),
        sessionId: "session-b" as SageAgentSessionId,
      }),
    ).toBe("requiresBootstrap");
    expect(useAcpProjectionStore.getState().takeProjectionChange()).toEqual(
      notice("9"),
    );
    expect(useAcpProjectionStore.getState().takeProjectionChange()).toBeNull();
  });
});

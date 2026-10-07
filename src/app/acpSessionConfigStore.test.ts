import { describe, expect, it } from "vitest";
import type { AcpChatPage, AgentSessionProjection } from "../domain/acp";
import {
  agentSessionFixture,
  capabilitySetFixture,
} from "../domain/acpProjection.fixture";
import {
  createAcpProjectionStore,
  type AcpProjectionScope,
} from "./acpProjectionStore";
import { safeAcpMutationResponse } from "../services/acpDesktopResponses";

const scope = {
  projectId: "project-a",
  projectGeneration: "7",
  chatId: "chat-a",
} as AcpProjectionScope;
const config = (revision = "1") => ({
  sageAgentSessionId: "session-a",
  runtimeEpoch: "2",
  configRevision: revision,
  modes: { currentModeId: "ask", availableModes: [{ id: "ask", name: "Ask" }] },
  configOptions: [
    {
      id: "effort",
      name: "Effort",
      type: "select" as const,
      category: "thought_level",
      currentValue: "high",
      options: [{ value: "high", name: "High" }],
    },
  ],
});
const tombstone = (revision = "4") => ({
  sageAgentSessionId: "session-a",
  runtimeEpoch: "2",
  configRevision: revision,
  removed: true,
  modes: null,
  configOptions: [],
});
function page(revision = "1"): AcpChatPage {
  return {
    projectId: "project-a",
    chatId: "chat-a",
    title: "A",
    projectionRevision: "10",
    currentSessionId: "session-a",
    sessionConfig: config(revision),
    sessions: [
      agentSessionFixture({
        state: "ready",
        residency: "hot",
        runtimeEpoch: "2" as AgentSessionProjection["runtimeEpoch"],
        projectionRevision:
          "10" as AgentSessionProjection["projectionRevision"],
        capabilities: capabilitySetFixture({
          protocolVersion: "acp-1",
          resumeSession: true,
          modelCount: 0,
          configCount: 1,
        }),
      }),
    ],
    turns: [],
    nextCursor: null,
  } as unknown as AcpChatPage;
}
const notice = (configRevision = "2") => ({
  schemaVersion: 1 as const,
  sageProjectId: "project-a",
  sageChatId: "chat-a",
  sageAgentSessionId: "session-a",
  runtimeEpoch: "2",
  configRevision,
});
function setup() {
  const store = createAcpProjectionStore();
  store.getState().activateScope(scope);
  store.getState().applyAuthoritativePage(scope, page());
  return store;
}

describe("ephemeral session config projection", () => {
  it("composes strict config with matching live owner and configCount", () => {
    expect(
      safeAcpMutationResponse({ projection: page() })?.projection.sessionConfig,
    ).toEqual(config());
    const absent = { ...page(), sessionConfig: null };
    expect(safeAcpMutationResponse({ projection: absent })).toBeNull();
    for (const patch of [
      { sageAgentSessionId: "other" },
      { runtimeEpoch: "3" },
    ])
      expect(
        safeAcpMutationResponse({
          projection: { ...page(), sessionConfig: { ...config(), ...patch } },
        }),
      ).toBeNull();
  });
  it("notices are independent bounded wake hints, not optimistic state", () => {
    const store = setup();
    const state = store.getState();
    expect(state.enqueueSessionConfigChange(scope, notice())).toBe("queued");
    expect(state.enqueueSessionConfigChange(scope, notice("3"))).toBe(
      "coalesced",
    );
    expect(state.enqueueSessionConfigChange(scope, notice())).toBe(
      "ignoredStale",
    );
    expect(store.getState().page?.sessionConfig?.configRevision).toBe("1");
    expect(store.getState().pendingChange).toBeNull();
    expect(state.takeSessionConfigChange()?.configRevision).toBe("3");
    expect(state.takeSessionConfigChange()).toBeNull();
  });
  it("rejects foreign UI generation, chat, session and epoch notices", () => {
    const state = setup().getState();
    expect(
      state.enqueueSessionConfigChange(
        { ...scope, projectGeneration: "8" as typeof scope.projectGeneration },
        notice(),
      ),
    ).toBe("ignoredStale");
    for (const patch of [
      { sageChatId: "other" },
      { sageProjectId: "other" },
      { sageAgentSessionId: "other" },
      { runtimeEpoch: "1" },
    ])
      expect(
        state.enqueueSessionConfigChange(scope, { ...notice(), ...patch }),
      ).toBe("ignoredStale");
  });
  it("rejects stale durable pages before independently preserving newer config", () => {
    const store = setup();
    store.getState().applyAuthoritativePage(scope, page("3"));
    store.getState().applyAuthoritativePage(scope, page("2"));
    expect(store.getState().page?.sessionConfig?.configRevision).toBe("3");
    expect(
      store.getState().applyAuthoritativePage(scope, {
        ...page("4"),
        projectionRevision: "9" as AcpChatPage["projectionRevision"],
      }),
    ).toBe("ignoredStaleRevision");
    expect(store.getState().page?.sessionConfig?.configRevision).toBe("3");
  });
  it.each(["authoritative", "load"] as const)(
    "preserves newer exact-owner config through delayed null/missing %s pages",
    (source) => {
      for (const state of ["ready", "active"] as const) {
        for (const missing of [false, true]) {
          const store = setup();
          const delayed = page();
          delayed.sessionConfig = null;
          if (missing) delete delayed.sessionConfig;
          delayed.sessions = delayed.sessions.map((session) => ({
            ...session,
            state,
            capabilities: { ...session.capabilities, configCount: 0 },
          }));
          store.getState().applyAuthoritativePage(scope, page("3"));
          const intent = store.getState().beginPageLoad(null);
          const disposition =
            source === "authoritative"
              ? store.getState().applyAuthoritativePage(scope, delayed)
              : store.getState().completePageLoad(intent, delayed);
          expect(disposition).toBe("accepted");
          expect(store.getState().page?.sessionConfig).toEqual(config("3"));
          expect(
            store.getState().page?.sessions[0].capabilities.configCount,
          ).toBe(1);
        }
      }
    },
  );
  it.each(["session", "epoch", "recovery", "cold", "no-current"] as const)(
    "clears newer live config on a %s transition",
    (transition) => {
      const store = setup();
      store.getState().applyAuthoritativePage(scope, page("3"));
      const incoming = page();
      incoming.sessionConfig = null;
      if (transition === "session" || transition === "epoch") {
        incoming.projectionRevision = "11" as AcpChatPage["projectionRevision"];
      }
      if (transition === "session") {
        incoming.currentSessionId =
          "session-b" as AcpChatPage["currentSessionId"];
      }
      if (transition === "no-current") {
        incoming.currentSessionId = null;
        incoming.projectionRevision = "11" as AcpChatPage["projectionRevision"];
      }
      incoming.sessions = incoming.sessions.map((session) => ({
        ...session,
        id:
          transition === "session"
            ? ("session-b" as typeof session.id)
            : session.id,
        runtimeEpoch:
          transition === "epoch"
            ? ("3" as typeof session.runtimeEpoch)
            : session.runtimeEpoch,
        state: transition === "recovery" ? "recovering" : session.state,
        residency: transition === "cold" ? "cold" : session.residency,
        capabilities: { ...session.capabilities, configCount: 0 },
      }));
      expect(store.getState().applyAuthoritativePage(scope, incoming)).toBe(
        "accepted",
      );
      expect(store.getState().page?.sessionConfig).toBeNull();
      expect(store.getState().page?.sessions[0].capabilities.configCount).toBe(
        0,
      );
    },
  );
  it("accepts absent config at a newer durable revision for the same owner", () => {
    const store = setup();
    store.getState().applyAuthoritativePage(scope, page("3"));
    const incoming = page();
    incoming.projectionRevision = "11" as AcpChatPage["projectionRevision"];
    incoming.sessionConfig = null;
    incoming.sessions = incoming.sessions.map((session) => ({
      ...session,
      capabilities: { ...session.capabilities, configCount: 0 },
    }));
    expect(store.getState().applyAuthoritativePage(scope, incoming)).toBe(
      "accepted",
    );
    expect(store.getState().page?.sessionConfig).toBeNull();
    expect(store.getState().page?.sessions[0].capabilities.configCount).toBe(0);
  });
  it("accepts exact-owner config removal tombstones at the same durable revision", () => {
    const store = setup();
    store.getState().applyAuthoritativePage(scope, page("3"));
    const incoming = page();
    incoming.sessionConfig = tombstone();
    incoming.sessions = incoming.sessions.map((session) => ({
      ...session,
      capabilities: { ...session.capabilities, configCount: 0 },
    }));
    expect(store.getState().applyAuthoritativePage(scope, incoming)).toBe(
      "accepted",
    );
    expect(store.getState().page?.sessionConfig).toEqual(tombstone());
    expect(store.getState().page?.sessions[0].capabilities.configCount).toBe(0);
    expect(
      store.getState().enqueueSessionConfigChange(scope, notice("4")),
    ).toBe("ignoredStale");
  });
  it.each(["authoritative", "load"] as const)(
    "preserves a newer tombstone through delayed same-revision present %s pages",
    (source) => {
      const store = setup();
      const removed = page();
      removed.sessionConfig = tombstone();
      removed.sessions = removed.sessions.map((session) => ({
        ...session,
        capabilities: { ...session.capabilities, configCount: 0 },
      }));
      store.getState().applyAuthoritativePage(scope, removed);

      const delayed = page("3");
      const disposition =
        source === "authoritative"
          ? store.getState().applyAuthoritativePage(scope, delayed)
          : store
              .getState()
              .completePageLoad(store.getState().beginPageLoad(null), delayed);

      expect(disposition).toBe("accepted");
      expect(store.getState().page?.sessionConfig).toEqual(tombstone());
      expect(store.getState().page?.sessions[0].capabilities.configCount).toBe(
        0,
      );
    },
  );
  it("does not let a stale tombstone erase newer exact-owner configuration", () => {
    const store = setup();
    store.getState().applyAuthoritativePage(scope, page("5"));
    const delayed = page();
    delayed.sessionConfig = tombstone("4");
    delayed.sessions = delayed.sessions.map((session) => ({
      ...session,
      capabilities: { ...session.capabilities, configCount: 0 },
    }));

    expect(store.getState().applyAuthoritativePage(scope, delayed)).toBe(
      "accepted",
    );
    expect(store.getState().page?.sessionConfig).toEqual(config("5"));
    expect(store.getState().page?.sessions[0].capabilities.configCount).toBe(1);
  });
  it("clears overlay on recovery and never leaks across scope activation", () => {
    const store = setup();
    const recovery = page();
    store.getState().applyAuthoritativePage(scope, {
      ...recovery,
      sessionConfig: null,
      sessions: recovery.sessions.map((s) => ({
        ...s,
        state: "recovering",
        capabilities: { ...s.capabilities, configCount: 0 },
      })),
    });
    expect(store.getState().page?.sessionConfig).toBeNull();
    store
      .getState()
      .activateScope({ ...scope, chatId: "other" as typeof scope.chatId });
    expect(store.getState().page).toBeNull();
    expect(store.getState().pendingConfigChange).toBeNull();
  });
});

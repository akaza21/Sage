import {
  acpProjectionStoreFor,
  clearAcpProjectionRegistry,
} from "../../../src/app/acpProjectionRegistry";
import { useAgentArtifactAdmissionStore } from "../../../src/app/agentArtifactAdmissionStore";
import { useAgentArtifactFollowStore } from "../../../src/app/agentArtifactFollowStore";
import {
  pinAgentChatOwner,
  pinnedAgentChatOwner,
} from "../../../src/app/agentChatOwner";
import { useWorkbenchStore } from "../../../src/app/store";
import type {
  AcpChatPage,
  AcpCommandFailure,
  AcpProjectionChangedEvent,
} from "../../../src/domain/acp";
import {
  clearUnreadAcks,
  seenChatUnreadFingerprint,
} from "../../../src/features/chat/acp/chatUnread";
import { clearSessionRailAcks } from "../../../src/features/chat/acp/sessionRail";
import { AcpDesktopError } from "../../../src/services/acpDesktop";
import {
  appendTurn,
  chatId,
  JOURNEY_IDS,
  type FixtureState,
  type JourneyEvent,
  updateLatestTurn,
} from "./agentTurnJourneyModel";
import { MAX_SCRIPTED_EVENTS } from "./agentTurnJourneyLimits";
import { createJourneyFixtureState } from "./agentTurnJourneyWorkbench";
import { journeySnapshot } from "./agentTurnJourneySnapshot";

export { JOURNEY_IDS, type JourneyEvent } from "./agentTurnJourneyModel";
export { MAX_SCRIPTED_EVENTS } from "./agentTurnJourneyLimits";

let fixture: FixtureState | null = null;

function requireFixture(): FixtureState {
  if (!fixture) throw new Error("Turn journey fixture is not installed");
  return fixture;
}

function recordEvent(state: FixtureState, event: JourneyEvent): void {
  if (state.eventCount >= MAX_SCRIPTED_EVENTS) {
    throw new Error(`Scripted event limit ${MAX_SCRIPTED_EVENTS} exceeded`);
  }
  state.eventCount += 1;
  const owner =
    event.type === "noop"
      ? "noop"
      : "chat" in event
        ? event.chat
        : event.source;
  state.eventHistory.push(`${event.type}:${owner}`);
}

function publish(state: FixtureState, page: AcpChatPage): string {
  state.pages.set(page.chatId, structuredClone(page));
  const rail = journeySnapshot(
    state,
    state.activeProjectId,
    state.transitionEpoch,
  );
  useWorkbenchStore.setState((current) => ({
    projects: rail.projects,
    tabs: current.tabs.map((tab) => {
      const receipt = rail.tabs.find((entry) => entry.id === tab.id);
      return receipt ? { ...tab, status: receipt.status } : tab;
    }),
  }));
  const store = acpProjectionStoreFor(page.chatId);
  const scope = store.getState().scope;
  const disposition = scope
    ? store.getState().applyAuthoritativePage(scope, structuredClone(page))
    : "ignoredStaleIntent";
  const turn = page.turns.at(-1) ?? null;
  const session = page.sessions.find(
    (candidate) => candidate.id === page.currentSessionId,
  );
  state.publications.push({
    projectId: page.projectId,
    projectGeneration: scope?.projectGeneration ?? null,
    chatId: page.chatId,
    sessionId: page.currentSessionId,
    sessionRevision: session?.revision ?? null,
    turnId: turn?.id ?? null,
    turnRevision: turn?.revision ?? null,
    turnState: turn?.state ?? null,
    runtimeEpoch: turn?.runtimeEpoch ?? page.sessions[0]?.runtimeEpoch ?? null,
    projectionRevision: page.projectionRevision,
    targetProjectId: scope?.projectId ?? null,
    targetChatId: scope?.chatId ?? null,
    disposition,
  });
  const event: AcpProjectionChangedEvent = {
    projectId: page.projectId,
    chatId: page.chatId,
    sessionId: page.currentSessionId!,
    projectionRevision: page.projectionRevision,
  };
  for (const listener of state.listeners) listener(event);
  return disposition;
}

export async function installAgentTurnJourneyFixture(): Promise<void> {
  if (fixture) await disposeAgentTurnJourneyFixture();
  fixture = await createJourneyFixtureState();
  document.documentElement.dataset.turnJourneyFixture = "ready";
  await new Promise<void>((resolve) =>
    requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
  );
}

export async function runJourneyEvent(
  event: JourneyEvent,
): Promise<string | null> {
  const state = requireFixture();
  recordEvent(state, event);
  if (event.type === "noop") return null;
  if (event.type === "resolveSend") {
    const id = chatId(event.chat);
    const pending = state.pendingSends.get(id);
    if (!pending) throw new Error(`No pending send for ${event.chat}`);
    state.pendingSends.delete(id);
    const page = appendTurn(
      state.pages.get(id)!,
      pending.request.prompt,
      event.state,
    );
    publish(state, page);
    pending.resolve({ projection: structuredClone(page) });
    return null;
  }
  if (event.type === "rejectSend") {
    const id = chatId(event.chat);
    const pending = state.pendingSends.get(id);
    if (!pending) throw new Error(`No pending send for ${event.chat}`);
    state.pendingSends.delete(id);
    const failure: AcpCommandFailure =
      event.certainty === "definite"
        ? {
            kind: "conflict",
            resource: "projectionRevision",
            message: "The prompt was definitely not dispatched.",
          }
        : {
            kind: "recoveryRequired",
            message: "Delivery could not be confirmed; inspect this Chat.",
          };
    pending.reject(new AcpDesktopError(failure));
    return null;
  }
  if (event.type === "resolveCancel") {
    const id = chatId(event.chat);
    const pending = state.pendingCancels.get(id);
    if (!pending) throw new Error(`No pending cancel for ${event.chat}`);
    state.pendingCancels.delete(id);
    const page = updateLatestTurn(state.pages.get(id)!, "cancelling");
    publish(state, page);
    pending.resolve({ projection: structuredClone(page) });
    return null;
  }
  if (event.type === "publish") {
    const id = chatId(event.chat);
    const page = updateLatestTurn(
      state.pages.get(id)!,
      event.state,
      event.assistantText,
    );
    return publish(state, page);
  }
  if (event.type === "seedHistory") {
    if (!Number.isInteger(event.turns) || event.turns < 1 || event.turns > 12) {
      throw new Error("History seed must contain 1 to 12 Turns");
    }
    const id = chatId(event.chat);
    let page = state.pages.get(id)!;
    for (let index = 0; index < event.turns; index += 1) {
      page = appendTurn(page, `History prompt ${index + 1}`, "completed");
      page = updateLatestTurn(
        page,
        "completed",
        `History response ${index + 1}: ${"bounded text ".repeat(24)}`,
      );
    }
    return publish(state, page);
  }
  const source = state.pages.get(chatId(event.source))!;
  const targetStore = acpProjectionStoreFor(chatId(event.target));
  const targetScope = targetStore.getState().scope;
  if (!targetScope) return "ignoredStaleIntent";
  const disposition = targetStore
    .getState()
    .applyAuthoritativePage(targetScope, structuredClone(source));
  state.publications.push({
    projectId: source.projectId,
    projectGeneration: targetScope.projectGeneration,
    chatId: source.chatId,
    sessionId: source.currentSessionId,
    sessionRevision:
      source.sessions.find((session) => session.id === source.currentSessionId)
        ?.revision ?? null,
    turnId: source.turns.at(-1)?.id ?? null,
    turnRevision: source.turns.at(-1)?.revision ?? null,
    turnState: source.turns.at(-1)?.state ?? null,
    runtimeEpoch:
      source.turns.at(-1)?.runtimeEpoch ??
      source.sessions[0]?.runtimeEpoch ??
      null,
    projectionRevision: source.projectionRevision,
    targetProjectId: targetScope.projectId,
    targetChatId: targetScope.chatId,
    disposition,
  });
  for (const listener of state.listeners) {
    listener({
      projectId: source.projectId,
      chatId: source.chatId,
      sessionId: source.currentSessionId!,
      projectionRevision: source.projectionRevision,
    });
  }
  return disposition;
}

export function journeyAudit() {
  const state = requireFixture();
  const workbench = useWorkbenchStore.getState();
  const alphaProjection = acpProjectionStoreFor(JOURNEY_IDS.alpha).getState();
  const betaProjection = acpProjectionStoreFor(JOURNEY_IDS.beta).getState();
  const gammaProjection = acpProjectionStoreFor(JOURNEY_IDS.gamma).getState();
  return structuredClone({
    eventCount: state.eventCount,
    eventHistory: state.eventHistory,
    sendRequests: state.sendRequests,
    cancelRequests: state.cancelRequests,
    publications: state.publications,
    composerHistory: state.composerHistory,
    pendingCalls: state.pendingSends.size + state.pendingCancels.size,
    listenerCount: state.listeners.size,
    workbench: {
      projectId: workbench.project?.id ?? null,
      projectGeneration: workbench.projectGeneration,
      nativeTransitionEpoch: workbench.nativeTransitionEpoch,
      projectTransitioning: workbench.projectTransitioning,
      activeTabId: workbench.activeTabId,
      pinnedAgentChatOwner: pinnedAgentChatOwner(),
      selectedHarnessId: workbench.selectedHarnessId,
      selectedModelId: workbench.selectedModelId,
      models: workbench.models,
      harnessDiscoveryPhase: workbench.harnessDiscoveryPhase,
      alphaUi: workbench.chatUiById[JOURNEY_IDS.alpha] ?? null,
      betaUi: workbench.chatUiById[JOURNEY_IDS.beta] ?? null,
      alphaScope: alphaProjection.scope,
      betaScope: betaProjection.scope,
      gammaScope: gammaProjection.scope,
      betaProjectionChatId: betaProjection.page?.chatId ?? null,
      gammaProjection: gammaProjection.page
        ? {
            projectId: gammaProjection.page.projectId,
            chatId: gammaProjection.page.chatId,
            projectionRevision: gammaProjection.page.projectionRevision,
          }
        : null,
      betaSeenFingerprint: seenChatUnreadFingerprint(JOURNEY_IDS.beta) ?? null,
    },
  });
}

export async function disposeAgentTurnJourneyFixture(): Promise<void> {
  if (fixture) {
    fixture.disposeStoreSubscription();
    fixture.restore();
    fixture.listeners.clear();
    fixture.pendingSends.clear();
    fixture.pendingCancels.clear();
    fixture.pages.clear();
    fixture = null;
  }
  clearAcpProjectionRegistry();
  clearUnreadAcks();
  clearSessionRailAcks();
  useAgentArtifactAdmissionStore.getState().clear();
  useAgentArtifactFollowStore.getState().clear();
  pinAgentChatOwner(null);
  useWorkbenchStore.setState({
    phase: "ready",
    error: null,
    projects: [],
    project: null,
    tabs: [],
    archivedChats: [],
    activeTabId: null,
    projectTransitioning: false,
    chatUiById: {},
    composerText: "",
    sending: false,
    chatError: null,
    messages: [],
  });
  delete document.documentElement.dataset.turnJourneyFixture;
}

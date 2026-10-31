import type { SageProjectId } from "../../../src/domain/acp";
import { getWorkbenchPorts } from "../../../src/ports/workbenchRuntime";
import { AcpDesktopError, acpDesktop } from "../../../src/services/acpDesktop";
import { desktop } from "../../../src/services/desktop";
import {
  JOURNEY_IDS,
  makeDeferred,
  MAX_CAPTURED_REQUESTS,
  MAX_DEFERRED_CALLS,
  MAX_PROJECTION_LISTENERS,
  type FixtureState,
} from "./agentTurnJourneyModel";
import { journeySnapshot } from "./agentTurnJourneySnapshot";
import {
  installJourneyAttention,
  journeyDetailPage,
} from "./agentTurnJourneyAttention";

function rejectWhenFull(total: number): void {
  if (total >= MAX_CAPTURED_REQUESTS) {
    throw new AcpDesktopError({
      kind: "capacity",
      resource: "queue",
      limit: MAX_CAPTURED_REQUESTS,
      message: "Journey request capture limit reached.",
    });
  }
}

export function installJourneyServices(state: FixtureState): () => void {
  const restoreAttention = installJourneyAttention(state);
  const originalAcp = { ...acpDesktop };
  const originalDesktopListHarnessModels = desktop.listHarnessModels;
  const ports = getWorkbenchPorts();
  const originalPorts = {
    getBootstrapSnapshot: ports.project.getBootstrapSnapshot,
    persistProjectLayout: ports.project.persistProjectLayout,
    switchProject: ports.project.switchProject,
    activateChat: ports.chat.activateChat,
    listHarnessModels: ports.agent.listHarnessModels,
    discoverProjectRepositories: ports.repository.discoverProjectRepositories,
  };

  acpDesktop.getChatPage = async (request) => {
    const page = state.pages.get(request.chatId);
    if (!page || page.projectId !== request.projectId) {
      throw new Error("Journey page request crossed Project or Chat ownership");
    }
    return journeyDetailPage(page, !request.cursor);
  };
  acpDesktop.subscribeProjectionChanges = async (listener) => {
    if (state.listeners.size >= MAX_PROJECTION_LISTENERS) {
      throw new Error(
        `Projection listener limit ${MAX_PROJECTION_LISTENERS} reached`,
      );
    }
    state.listeners.add(listener);
    return () => state.listeners.delete(listener);
  };
  acpDesktop.sendTurn = (request) => {
    rejectWhenFull(state.sendRequests.length + state.cancelRequests.length);
    if (
      state.pendingSends.size + state.pendingCancels.size >=
      MAX_DEFERRED_CALLS
    ) {
      return Promise.reject(
        new Error(`Deferred call limit ${MAX_DEFERRED_CALLS} reached`),
      );
    }
    if (state.pendingSends.has(request.chatId)) {
      return Promise.reject(
        new Error("Duplicate fixture send was not admitted"),
      );
    }
    state.sendRequests.push(structuredClone(request));
    const pending = { ...makeDeferred(), request };
    state.pendingSends.set(request.chatId, pending);
    return pending.promise;
  };
  acpDesktop.cancelTurn = (request) => {
    rejectWhenFull(state.sendRequests.length + state.cancelRequests.length);
    if (
      state.pendingSends.size + state.pendingCancels.size >=
      MAX_DEFERRED_CALLS
    ) {
      return Promise.reject(
        new Error(`Deferred call limit ${MAX_DEFERRED_CALLS} reached`),
      );
    }
    state.cancelRequests.push(structuredClone(request));
    const pending = { ...makeDeferred(), request };
    state.pendingCancels.set(request.chatId, pending);
    return pending.promise;
  };

  ports.project.getBootstrapSnapshot = async () =>
    structuredClone(
      journeySnapshot(state, state.activeProjectId, state.transitionEpoch),
    );
  ports.project.persistProjectLayout = async () => undefined;
  ports.project.switchProject = async (request, expectedEpoch) => {
    state.activeProjectId = request.targetProjectId as SageProjectId;
    state.transitionEpoch = expectedEpoch + 1;
    return {
      committed: true,
      transitionEpoch: state.transitionEpoch,
      snapshot: structuredClone(
        journeySnapshot(state, state.activeProjectId, state.transitionEpoch),
      ),
      recoveryError: null,
    };
  };
  ports.chat.activateChat = async (layout, expectedEpoch) => {
    state.activeChatByProject.set(
      state.activeProjectId,
      layout.activeTabId as never,
    );
    state.transitionEpoch = expectedEpoch + 1;
    return {
      committed: true,
      transitionEpoch: state.transitionEpoch,
      snapshot: structuredClone(
        journeySnapshot(state, state.activeProjectId, state.transitionEpoch),
      ),
      recoveryError: null,
    };
  };
  ports.agent.listHarnessModels = async () => [
    { id: JOURNEY_IDS.model, name: "Journey model", isDefault: true },
  ];
  desktop.listHarnessModels = ports.agent.listHarnessModels;
  ports.repository.discoverProjectRepositories = async (request) => ({
    projectId: request.projectId,
    transitionEpoch: request.expectedTransitionEpoch,
    refs: [],
    truncated: false,
  });

  return () => {
    restoreAttention();
    Object.assign(acpDesktop, originalAcp);
    ports.project.getBootstrapSnapshot = originalPorts.getBootstrapSnapshot;
    ports.project.persistProjectLayout = originalPorts.persistProjectLayout;
    ports.project.switchProject = originalPorts.switchProject;
    ports.chat.activateChat = originalPorts.activateChat;
    ports.agent.listHarnessModels = originalPorts.listHarnessModels;
    desktop.listHarnessModels = originalDesktopListHarnessModels;
    ports.repository.discoverProjectRepositories =
      originalPorts.discoverProjectRepositories;
  };
}

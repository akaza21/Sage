import {
  acpProjectionStoreFor,
  clearAcpProjectionRegistry,
  pinAcpProjectionIds,
} from "../../../src/app/acpProjectionRegistry";
import { useAgentArtifactAdmissionStore } from "../../../src/app/agentArtifactAdmissionStore";
import { useAgentArtifactFollowStore } from "../../../src/app/agentArtifactFollowStore";
import { clearBrowseSessions } from "../../../src/features/preview/harnessBrowse";
import { acpDesktop } from "../../../src/services/acpDesktop";

import { buildAgentPages } from "./surfaces/agentVisualFixtures";
import { buildEditorPages } from "./surfaces/editorVisualFixtures";
import { buildRecoveryPages } from "./surfaces/recoveryVisualFixtures";
import { buildResourcePages } from "./surfaces/resourceVisualFixtures";
import {
  fixtureTabs,
  visualSnapshotDiffRef,
  visualSnapshotNewText,
  visualSnapshotOldText,
} from "./visualDocumentFixtures";
import { installEditorToolFixtures } from "./surfaces/editorVisualFixtures";
import { GENERATION, PROJECT_ID } from "./visualFixtureIds";
import { VISUAL_SCENARIOS } from "./visualScenarioCatalog";
import type {
  BuiltVisualScenario,
  VisualScenarioName,
} from "./visualScenarioTypes";
import {
  applyVisualWorkbenchState,
  reapplyVisualPreviewBrowseIntent,
} from "./visualWorkbenchState";

const defaultGetFileSnapshot = acpDesktop.getFileSnapshot;

export function buildVisualScenario(
  name: VisualScenarioName,
): BuiltVisualScenario {
  const definition = VISUAL_SCENARIOS.find((entry) => entry.name === name);
  if (!definition) throw new Error(`Unknown visual scenario: ${name}`);
  const pages =
    buildAgentPages(name) ??
    buildEditorPages(name) ??
    buildResourcePages(name) ??
    buildRecoveryPages(name);
  if (!pages) throw new Error(`Unknown visual scenario: ${name}`);
  return { ...definition, pages };
}

function isVisualSnapshotRequest(
  request: Parameters<typeof acpDesktop.getFileSnapshot>[0],
): boolean {
  return (
    request.projectGeneration === GENERATION &&
    request.expectedProjectionRevision === "42" &&
    Object.entries(visualSnapshotDiffRef).every(
      ([key, value]) =>
        request[key as keyof typeof visualSnapshotDiffRef] === value,
    )
  );
}

function installVisualSnapshotBridge(name: VisualScenarioName): void {
  if (name !== "diff") return;
  acpDesktop.getFileSnapshot = async (request) => {
    if (!isVisualSnapshotRequest(request)) {
      throw new Error("Visual snapshot fixture rejected an unrecognized ref.");
    }
    return {
      ...visualSnapshotDiffRef,
      projectionRevision: "42" as never,
      oldText: visualSnapshotOldText,
      newText: visualSnapshotNewText,
      oldBytes: 54,
      newBytes: 54,
    };
  };
}

export async function installVisualScenario(name: VisualScenarioName) {
  const scenario = buildVisualScenario(name);
  acpDesktop.getFileSnapshot = defaultGetFileSnapshot;
  const tabs = fixtureTabs(scenario);
  const chatIds = scenario.pages.map((pageValue) => pageValue.chatId);

  clearAcpProjectionRegistry();
  clearBrowseSessions();
  useAgentArtifactAdmissionStore.getState().clear();
  useAgentArtifactFollowStore.getState().clear();
  pinAcpProjectionIds(chatIds);
  installEditorToolFixtures();

  const pagesByChat = new Map(
    scenario.pages.map((pageValue) => [pageValue.chatId, pageValue]),
  );
  installVisualSnapshotBridge(name);
  acpDesktop.getChatPage = async (request) => {
    const pageValue = pagesByChat.get(request.chatId);
    if (!pageValue) throw new Error(`No visual page for ${request.chatId}`);
    return structuredClone(pageValue);
  };

  for (const pageValue of scenario.pages) {
    const scope = {
      projectId: PROJECT_ID,
      projectGeneration: GENERATION,
      chatId: pageValue.chatId,
    };
    const store = acpProjectionStoreFor(pageValue.chatId);
    store.getState().activateScope(scope);
    store.getState().applyAuthoritativePage(scope, structuredClone(pageValue));
  }

  applyVisualWorkbenchState(scenario, tabs);

  document.documentElement.dataset.visualScenario = name;
  await new Promise((resolve) =>
    requestAnimationFrame(() => resolve(undefined)),
  );
  await reapplyVisualPreviewBrowseIntent(scenario);
}

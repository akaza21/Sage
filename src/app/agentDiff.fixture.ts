import type {
  AcpChatPage,
  AcpItem,
  ProjectGeneration,
  TurnProjection,
} from "../domain/acp";
import { parseAgentDiffRef } from "../domain/agentDiff";
import { acpProjectionStoreFor } from "./acpProjectionRegistry";
import { useWorkbenchStore } from "./store";

export const snapshotResource = { rootId: "root-a", relativePath: "src/a.ts" };
export const snapshotRef = parseAgentDiffRef({
  projectId: "project-a",
  chatId: "chat-a",
  sessionId: "session-a",
  runtimeEpoch: "3",
  turnId: "turn-a",
  itemId: "item-a",
  diffOrdinal: 0,
  snapshotRevision: "1",
  ...snapshotResource,
  contentDigest: "a".repeat(64),
})!;
export function snapshotPage(): AcpChatPage {
  const item = {
    id: snapshotRef.itemId,
    ...snapshotRef,
    ordinal: 1,
    origin: "primary",
    activityId: null,
    lifecycle: "completed",
    revision: "1",
    createdAt: 1,
    updatedAt: 1,
    terminalAt: 1,
    kind: "fileChangeDiff",
    toolState: null,
    payload: {
      truncated: false,
      changes: [
        {
          resource: { kind: "resource", resource: snapshotResource },
          originalResource: null,
          operation: "updated",
          summary: "Saved edit",
          additions: 1,
          deletions: 1,
          pairState: "available",
          snapshotRevision: snapshotRef.snapshotRevision,
          contentDigest: snapshotRef.contentDigest,
          oldBytes: 4,
          newBytes: 4,
        },
      ],
    },
  } as unknown as AcpItem;
  const turn = {
    id: snapshotRef.turnId,
    sessionId: snapshotRef.sessionId,
    runtimeEpoch: "3",
    ordinal: 1,
    state: "completed",
    revision: "1",
    createdAt: 1,
    updatedAt: 1,
    terminalAt: 1,
    items: [item],
    activities: [],
  } as unknown as TurnProjection;
  return {
    projectId: snapshotRef.projectId,
    chatId: snapshotRef.chatId,
    title: "Chat",
    projectionRevision: "1",
    currentSessionId: snapshotRef.sessionId,
    sessions: [{ id: snapshotRef.sessionId, ordinal: 1 }],
    turns: [turn],
    nextCursor: null,
  } as unknown as AcpChatPage;
}
export function seedSnapshotOwner() {
  const page = snapshotPage();
  const scope = {
    projectId: page.projectId,
    chatId: page.chatId,
    projectGeneration: "1" as ProjectGeneration,
  };
  const projection = acpProjectionStoreFor(page.chatId);
  projection.getState().activateScope(scope);
  projection.getState().applyAuthoritativePage(scope, page);
  useWorkbenchStore.setState({
    project: {
      id: page.projectId,
      name: "Project",
      roots: [
        {
          id: snapshotResource.rootId,
          name: "Root",
          ordinal: 0,
          availability: "available",
          message: null,
        },
      ],
    },
    projectGeneration: 1,
    nativeTransitionEpoch: 1,
    projectTransitioning: false,
    tabs: [],
    activeTabId: null,
    diffs: {},
    requestGenerations: {},
    requestIntentSequence: 0,
  });
  return projection;
}

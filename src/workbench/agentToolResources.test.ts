import { expect, it } from "vitest";
import type { AcpChatPage } from "../domain/acp";
import type {
  AgentArtifactAdmission,
  AgentArtifactOwner,
} from "../domain/agentArtifact";
import type { WorkbenchTab } from "../domain/workbench";
import { agentToolResources } from "./agentToolResources";

const owner = {
  projectId: "project-a",
  chatId: "chat-a",
  sessionId: "session-a",
} as AgentArtifactOwner;
const page = {
  ...owner,
  currentSessionId: owner.sessionId,
  turns: [],
} as unknown as AcpChatPage;
const tabs: WorkbenchTab[] = ["file", "diff", "file"].map((kind, index) => ({
  id: `tab-${index}`,
  kind,
  title: `File ${index}`,
  resource: { rootId: "root-a", relativePath: `src/file-${index}.ts` },
  position: index,
  pinned: false,
  dirty: false,
})) as WorkbenchTab[];
const admissions: AgentArtifactAdmission[] = tabs.map((tab, index) => ({
  tabId: tab.id,
  owner:
    index === 2
      ? ({ ...owner, chatId: "chat-other" } as AgentArtifactOwner)
      : owner,
}));
it("opens files from real edit projections but leaves unsaved diffs and deleted files unavailable", () => {
  const turn = {
    id: "turn-a",
    sessionId: owner.sessionId,
    runtimeEpoch: "3",
    ordinal: 1,
    items: ["updated", "deleted"].map((operation, ordinal) => ({
      ...owner,
      id: `item-${ordinal}`,
      turnId: "turn-a",
      runtimeEpoch: "3",
      ordinal,
      origin: "primary",
      activityId: null,
      lifecycle: "completed",
      revision: "1",
      createdAt: 1,
      updatedAt: 2,
      terminalAt: 2,
      kind: "fileChangeDiff",
      payload: {
        truncated: false,
        changes: [
          {
            resource: {
              kind: "resource",
              resource: { rootId: "root-a", relativePath: `${operation}.ts` },
            },
            operation,
            originalResource: null,
            summary: operation,
            additions: null,
            deletions: null,
          },
        ],
      },
    })),
  };
  const projection = {
    ...page,
    sessions: [{ id: owner.sessionId }],
    turns: [turn],
  } as unknown as AcpChatPage;
  const files = agentToolResources("files", projection, owner, [], []);
  expect(files.map((entry) => entry.label)).toEqual([
    "updated.ts",
    "deleted.ts",
  ]);
  expect(files[0].intent).toMatchObject({ owner, target: "file" });
  expect(files[1].intent).toBeUndefined();
  expect(
    agentToolResources("changes", projection, owner, [], []).every(
      (entry) => !entry.intent,
    ),
  ).toBe(true);
});
it("quick-opens only the current Chat's admitted files, not foreign resources", () => {
  expect(agentToolResources("files", page, owner, tabs, admissions)).toEqual([
    {
      resource: { rootId: "root-a", relativePath: "src/file-0.ts" },
      label: "src/file-0.ts",
      tabId: "tab-0",
    },
  ]);
  expect(agentToolResources("changes", page, owner, tabs, admissions)).toEqual([
    {
      resource: { rootId: "root-a", relativePath: "src/file-1.ts" },
      label: "src/file-1.ts",
      tabId: "tab-1",
    },
  ]);
});
it("fails closed with no session, stale session, or another Chat's page", () => {
  expect(agentToolResources("files", page, null, tabs, admissions)).toEqual([]);
  expect(
    agentToolResources(
      "changes",
      { ...page, currentSessionId: "old-session" } as AcpChatPage,
      owner,
      tabs,
      admissions,
    ),
  ).toEqual([]);
  expect(
    agentToolResources(
      "files",
      { ...page, chatId: "other-chat" } as AcpChatPage,
      owner,
      tabs,
      admissions,
    ),
  ).toEqual([]);
});
it("keeps an overlarge admission input bounded to its latest 32 entries", () => {
  const manyTabs = Array.from({ length: 1000 }, (_, index) => ({
    ...tabs[0],
    id: `file-${index}`,
    resource: { rootId: "root-a", relativePath: `file-${index}.ts` },
  }));
  const manyAdmissions = manyTabs.map((tab) => ({ tabId: tab.id, owner }));
  const result = agentToolResources(
    "files",
    page,
    owner,
    manyTabs,
    manyAdmissions,
  );
  expect(result).toHaveLength(32);
  expect(result[0].label).toBe("file-968.ts");
});

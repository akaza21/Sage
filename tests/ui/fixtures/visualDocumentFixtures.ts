import type { AcpChatPage } from "../../../src/domain/acp";
import type {
  EditorDocument,
  FileDiff,
  ResourceRef,
  WorkbenchTab,
} from "../../../src/domain/workbench";
import type { AgentDiffRef } from "../../../src/domain/agentDiff";
import type {
  SageChatId,
  SageItemId,
  SageTurnId,
} from "../../../src/domain/acp";

import type { BuiltVisualScenario } from "./visualScenarioTypes";
import {
  FIXED_NOW,
  PROJECT_ID,
  ROOT_ID,
  RUNTIME_EPOCH,
  SESSION_ID,
} from "./visualFixtureIds";

export const fileResource: ResourceRef = {
  rootId: ROOT_ID,
  relativePath: "src/workbench/AgentChatPanes.tsx",
};

export const editorContent = [
  'import { ChatSurface } from "../features/chat/ChatSurface";',
  "",
  "export function AgentChatPanes() {",
  "  const visibleChats = useVisibleChats({ maximum: 2 });",
  "",
  "  return visibleChats.map((chat) => (",
  "    <ChatSurface key={chat.id} chatId={chat.id} />",
  "  ));",
  "}",
].join("\n");

export function editorDocument(): EditorDocument {
  const version = {
    fingerprint: "visual-editor-v1",
    byteLen: editorContent.length,
    modifiedAt: FIXED_NOW - 90_000,
  };
  return {
    resource: fileResource,
    disk: version,
    base: version,
    buffer: {
      ...version,
      revision: 0,
      content: editorContent,
    },
    lineEnding: "lf",
    dirty: false,
    conflict: null,
  };
}

export const visualSnapshotOldText =
  "const visibleChats = useVisibleChats({ maximum: 1 });\n";
export const visualSnapshotNewText =
  "const visibleChats = useVisibleChats({ maximum: 2 });\n";

/** Exact immutable card fixture identity; the visual bridge admits no other ref. */
export const visualSnapshotDiffRef: AgentDiffRef = {
  projectId: PROJECT_ID,
  chatId: "visual-chat-diff" as SageChatId,
  sessionId: SESSION_ID,
  runtimeEpoch: RUNTIME_EPOCH,
  turnId: "visual-turn-visual-chat-diff-1" as SageTurnId,
  itemId: "visual-turn-visual-chat-diff-1-item-6" as SageItemId,
  diffOrdinal: 0,
  snapshotRevision: "1",
  ...fileResource,
  contentDigest:
    "627b6f10d44396419682846de4a005bab4a57833b76d9710bdd223bd2cfa8597",
};

export const visualDiff: FileDiff = {
  resource: fileResource,
  binary: false,
  truncated: false,
  hunks: [
    {
      header: "@@ -18,7 +18,10 @@ export function AgentChatPanes()",
      lines: [
        {
          kind: "context",
          oldLine: 18,
          newLine: 18,
          text: "const chats = visibleChats();",
        },
        {
          kind: "removed",
          oldLine: 19,
          newLine: null,
          text: "return <ChatSurface chatId={chats[0].id} />;",
        },
        {
          kind: "added",
          oldLine: null,
          newLine: 19,
          text: "return chats.slice(0, 2).map((chat) => (",
        },
        {
          kind: "added",
          oldLine: null,
          newLine: 20,
          text: "  <ChatSurface key={chat.id} chatId={chat.id} />",
        },
        { kind: "added", oldLine: null, newLine: 21, text: "));" },
      ],
    },
  ],
};

function chatTab(pageValue: AcpChatPage, position: number): WorkbenchTab {
  const nonterminal = pageValue.turns.some((candidate) =>
    ["running", "waitingTool", "waitingApproval", "waitingModel"].includes(
      candidate.state,
    ),
  );
  return {
    id: pageValue.chatId,
    kind: "chat",
    title: pageValue.title,
    position,
    pinned: position === 0,
    dirty: false,
    status: nonterminal ? "working" : "complete",
    updatedAt: FIXED_NOW - position * 60_000,
  };
}

export function fixtureTabs(scenario: BuiltVisualScenario): WorkbenchTab[] {
  const chats = scenario.pages.map(chatTab);
  if (scenario.resource === "file") {
    return [
      ...chats,
      {
        id: "visual-file-tab",
        kind: "file",
        title: "AgentChatPanes.tsx",
        position: chats.length,
        pinned: false,
        dirty: false,
        resource: fileResource,
      },
    ];
  }
  if (scenario.resource === "diff") {
    // The agent-diff visual route must begin at the FileChange card. A tab or
    // Git-style cache seeded here would bypass immutable snapshot admission.
    return chats;
  }
  return chats;
}

import type { AcpChatPage } from "../../../src/domain/acp";
import type {
  EditorDocument,
  FileDiff,
  ResourceRef,
  WorkbenchTab,
} from "../../../src/domain/workbench";

import type { BuiltVisualScenario } from "./visualScenarioTypes";
import { FIXED_NOW, ROOT_ID } from "./visualFixtureIds";

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
    return [
      ...chats,
      {
        id: "visual-diff-tab",
        kind: "diff",
        title: "AgentChatPanes.tsx",
        position: chats.length,
        pinned: false,
        dirty: false,
        resource: fileResource,
      },
    ];
  }
  return chats;
}

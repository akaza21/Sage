import { useRef } from "react";
import { createRoot } from "react-dom/client";
import { useWorkbenchStore } from "../../../src/app/store";
import { AgentTaskHeader } from "../../../src/workbench/AgentTaskHeader";
import { ProjectChatNavigation } from "../../../src/workbench/ProjectChatNavigation";
import { ChatTranscript } from "../../../src/features/chat/ChatTranscript";
import { ChatHome } from "../../../src/features/chat/ChatHome";
import type { AcpChatPage } from "../../../src/domain/acp";
import type { ChatLiveStatusTone } from "../../../src/features/chat/acp/chatLiveSnapshot";
import { useFollowEnd } from "../../../src/features/chat/useFollowEnd";
import {
  page,
  textItem,
} from "../../../src/features/chat/acp/acpProjection.fixture";
import "../../../src/styles/searchable-picker-bounded-popover.css";
import "../../../src/styles/global.css";
import "../../../src/styles/shared-controls.css";
import "../../../src/styles/project-navigation-surface.css";
import "../../../src/styles/chat-navigation-surface.css";
import "../../../src/styles/chat-surface.css";
import "../../../src/styles/agent-chat-panes.css";
import "../../../src/styles/composer-surface.css";
import "../../../src/styles/chat-container-controls.css";

const tab = {
  id: "chat:chrome",
  kind: "chat" as const,
  title: "A long chat title that must truncate instead of wrapping the header",
  position: 0,
  pinned: false,
  dirty: false,
};
const project = {
  id: "project:chrome",
  name: "Sage project with a long name",
  roots: [],
};
const params = new URLSearchParams(location.search);
const harnessId = params.get("harness") ?? "codex";
const home = params.get("state")?.startsWith("home") ?? false;
const split = params.get("state") === "home-split";
const statusLabel = params.get("status") ?? "Completed";
const harnessStatus = params.get("harnessStatus") ?? "Live";
const statusTone = (params.get("tone") ?? "idle") as ChatLiveStatusTone;
const chatPage: AcpChatPage = {
  ...page("completed"),
  projectId: project.id as never,
  chatId: tab.id as never,
};
chatPage.turns[0].items = [
  {
    ...textItem(),
    id: "user:chrome" as never,
    payload: {
      role: "user",
      text: "Please inspect the chat chrome carefully.\nKeep every wrapped user prompt rectangular.\nCheck all supported harnesses at narrow widths.",
    },
  },
  {
    ...textItem(),
    payload: {
      role: "assistant",
      text: Array.from(
        { length: 24 },
        (_, index) =>
          `Paragraph ${index + 1}: A bounded transcript exercises scrolling and the latest-turn control.`,
      ).join("\n\n"),
    },
  },
];
if (home) chatPage.turns = [];
useWorkbenchStore.setState({
  project,
  projects: [{ ...project, rootCount: 0, lastOpenedAt: 1 }],
  tabs: [tab],
  activeTabId: tab.id,
  selectedHarnessId: harnessId,
  archivedChats: [{ id: "chat:old", title: "Archived research" }],
});

export function ChromeFixture() {
  const transcriptRef = useRef<HTMLDivElement>(null);
  const contentRef = useRef<HTMLDivElement>(null);
  const endRef = useRef<HTMLDivElement>(null);
  const follow = useFollowEnd({
    ownerKey: tab.id,
    revision: chatPage.projectionRevision,
    transcriptRef,
    contentRef,
    endRef,
  });
  return (
    <div className="chrome-layout" data-harness={harnessId}>
      <aside>
        <ProjectChatNavigation projectId={project.id} chats={[tab]} selected />
      </aside>

      <section className="agent-chat-center">
        <AgentTaskHeader
          tab={tab}
          chatCount={1}
          snapshot={{
            chatId: tab.id,
            statusLabel,
            statusTone,
            latestText: "",
            streaming: false,
            waiting: statusTone === "waiting",
            working: statusTone === "working",
          }}
          sessionChrome={{
            chatId: tab.id,
            harnessStatus,
            canCloseSession: false,
            canEndSession: false,
            busy: false,
            acpPage: null,
            messages: [],
            actions: {
              current: {
                closeSession: () => undefined,
                endSession: () => undefined,
                focusComposer: () => undefined,
              },
            },
          }}
        />
        <div
          className={`agent-chat-panes${split ? " agent-chat-panes--split" : ""}`}
        >
          <div className="agent-chat-pane">
            <section className="chat-surface chat-surface--agent">
              <ChatTranscript
                transcriptRef={transcriptRef}
                contentRef={contentRef}
                endRef={endRef}
                onScroll={follow.updateFollowEnd}
                projectTransitioning={false}
                project={project}
                useNativeAcp
                page={chatPage}
                focused
                projectRoots={[]}
                retentionIncomplete={false}
                onOpenResource={() => undefined}
                hasEarlier={false}
                loadingEarlier={false}
                historyError={null}
                onLoadEarlier={() => undefined}
                messages={[]}
                developmentSimulator={false}
                showJumpToLatest={follow.showJumpToLatest}
                onJumpToLatest={follow.jumpToLatest}
              />
              <form className="composer" aria-label="Message composer" />
            </section>
          </div>
          {split ? (
            <>
              <div className="agent-chat-splitter" />
              <div className="agent-chat-pane">
                <ChatHome projectName={project.name} />
              </div>
            </>
          ) : null}
        </div>
      </section>
    </div>
  );
}

createRoot(document.getElementById("root")!).render(<ChromeFixture />);

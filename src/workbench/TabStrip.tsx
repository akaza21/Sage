import {
  GitCompareArrows,
  Globe2,
  MessageSquare,
  Plus,
  Settings,
  TerminalSquare,
  X,
} from "../ui/leanIcons";

import { useRef } from "react";

import type { TabKind } from "../domain/workbench";
import { orderedTabs } from "../app/tabState";
import { useWorkbenchStore } from "../app/store";
import { useEditorSettingsStore } from "../app/editorSettingsStore";
import { FileKindIcon } from "../features/files/FileKindIcon";
import { useBrowserAgentPresence } from "../app/browserAgentActivityStore";
import type { WorkbenchTab } from "../domain/workbench";
import { BrowserFavicon } from "../features/browser/browserFavicon";
import {
  resourceHasGitMark,
  useGitDecorationsUi,
} from "../features/git/gitDecorationsGate";
import type { ResourceRef } from "../domain/workbench";
import { useResourceTabStrip } from "./useResourceTabStrip";

function BrowserTabPresence({
  tabId,
  projectId,
  tabs,
}: {
  tabId: string;
  projectId: string | null;
  tabs: readonly WorkbenchTab[];
}) {
  const operation = useBrowserAgentPresence(projectId, tabId);
  if (!operation) return null;
  const chatTitle =
    tabs.find((tab) => tab.kind === "chat" && tab.id === operation.chatId)
      ?.title ?? "Chat";
  const status =
    operation.phase === "started"
      ? "Agent has a browser action pending"
      : "Agent used this tab";
  return (
    <span className="tab__agent-presence" title={`${status} · ${chatTitle}`}>
      <span aria-hidden="true">Agent · {chatTitle}</span>
      <span className="sr-only">
        {status} · {chatTitle}
      </span>
    </span>
  );
}

/** The room a File tab's Git mark will take, held while the marks load. */
function PendingGitMark({ resource }: { resource: ResourceRef }) {
  const marked = useWorkbenchStore((state) =>
    resourceHasGitMark(state.repositories, resource),
  );
  return marked ? <span className="tab__git-slot" aria-hidden="true" /> : null;
}

const icons: Record<TabKind, typeof MessageSquare> = {
  chat: MessageSquare,
  file: MessageSquare,
  diff: GitCompareArrows,
  terminal: TerminalSquare,
  search: MessageSquare,
  problems: MessageSquare,
  output: TerminalSquare,
  settings: Settings,
  browser: Globe2,
};

export interface TabStripProps {
  kinds?: ReadonlyArray<TabKind>;
  showNewChat?: boolean;
  label?: string;
}

export function TabStrip({
  kinds,
  showNewChat = true,
  label = "Open tabs",
}: TabStripProps = {}) {
  const tabs = useWorkbenchStore((state) => state.tabs);
  const activeTabId = useWorkbenchStore((state) => state.activeTabId);
  const activateTab = useWorkbenchStore((state) => state.activateTab);
  const closeTab = useWorkbenchStore((state) => state.closeTab);
  const createChat = useWorkbenchStore((state) => state.createChat);
  const project = useWorkbenchStore((state) => state.project);
  const decorate = useEditorSettingsStore(
    (store) => store.settings["git.decorations"],
  );
  // The marks load only while on and a repository root is open.
  const wantGit = useWorkbenchStore(
    (state) =>
      decorate &&
      state.repositories.roots.some((root) => root.state === "repository"),
  );
  const git = useGitDecorationsUi(wantGit);
  const attachedChatCount = tabs.filter((tab) => tab.kind === "chat").length;
  const visibleTabs = kinds
    ? orderedTabs(tabs).filter((tab) => kinds.includes(tab.kind))
    : orderedTabs(tabs);
  const scrollRef = useRef<HTMLDivElement>(null);

  // Selecting a tab opens its resource, so arrowing past a tab only moves
  // focus; Enter or Space selects it.
  const onTabKeyDown = useResourceTabStrip(scrollRef, {
    activation: "manual",
    revealKey: activeTabId,
  });

  return (
    <nav className="tab-strip" aria-label={label}>
      <div
        className="tab-strip__scroll"
        ref={scrollRef}
        role="tablist"
        aria-label="Tabs"
        onKeyDown={onTabKeyDown}
      >
        {visibleTabs.map((tab) => {
          const Icon = icons[tab.kind];
          const active = tab.id === activeTabId;
          const closeDisabled = tab.kind === "chat" && attachedChatCount <= 1;

          return (
            <div
              className={`tab ${active ? "tab--active" : ""}`}
              key={tab.id}
              role="presentation"
              onMouseDown={(event) => {
                // Prevent WebView2 autoscroll before the auxiliary click arrives.
                if (event.button === 1) event.preventDefault();
              }}
              onAuxClick={(event) => {
                if (event.button !== 1) return;
                event.preventDefault();
                if (!tab.pinned && !closeDisabled) closeTab(tab.id);
              }}
            >
              <button
                className="tab__activate"
                type="button"
                role="tab"
                aria-selected={active}
                title={tab.resource?.relativePath ?? tab.title}
                onClick={() => void activateTab(tab.id)}
              >
                {tab.kind === "browser" ? (
                  <BrowserFavicon
                    key={tab.favicon ?? ""}
                    value={tab.favicon}
                    className="tab__favicon"
                    fallback={
                      <Globe2 size={16} strokeWidth={1.8} aria-hidden="true" />
                    }
                  />
                ) : tab.kind === "file" ? (
                  <FileKindIcon
                    path={tab.resource?.relativePath ?? tab.title}
                    size={16}
                  />
                ) : (
                  <Icon size={16} strokeWidth={1.8} aria-hidden="true" />
                )}
                {git && tab.kind === "file" && tab.resource ? (
                  <git.GitTabTitle title={tab.title} resource={tab.resource} />
                ) : (
                  <>
                    <span>{tab.title}</span>
                    {wantGit && tab.kind === "file" && tab.resource ? (
                      <PendingGitMark resource={tab.resource} />
                    ) : null}
                  </>
                )}
                {tab.kind === "browser" ? (
                  <BrowserTabPresence
                    tabId={tab.id}
                    projectId={project?.id ?? null}
                    tabs={tabs}
                  />
                ) : null}
                {tab.status === "working" ? (
                  <span className="tab__status" title="Working" />
                ) : null}
                {tab.dirty ? (
                  <span className="tab__dirty" title="Unsaved changes" />
                ) : null}
              </button>
              {!tab.pinned ? (
                <button
                  className="tab__close"
                  type="button"
                  aria-label={`${tab.kind === "chat" ? "Archive" : "Close"} ${tab.title}`}
                  disabled={closeDisabled}
                  onClick={() => closeTab(tab.id)}
                >
                  <X size={14} aria-hidden="true" />
                </button>
              ) : null}
            </div>
          );
        })}
      </div>
      {showNewChat ? (
        <button
          className="icon-button tab-strip__new"
          type="button"
          aria-label="New chat"
          title="New chat"
          disabled={!project}
          onClick={() => void createChat()}
        >
          <Plus size={17} aria-hidden="true" />
        </button>
      ) : null}
    </nav>
  );
}

export default TabStrip;

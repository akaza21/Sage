import {
  ChevronDown,
  ChevronRight,
  FolderOpen,
  FolderPlus,
  MessageSquare,
  Plus,
  Search,
  Settings,
  SquarePen,
} from "lucide-react";

import { useWorkbenchStore } from "../app/store";

export function LeftSidebar() {
  const projects = useWorkbenchStore((state) => state.projects);
  const project = useWorkbenchStore((state) => state.project);
  const tabs = useWorkbenchStore((state) => state.tabs);
  const activeTabId = useWorkbenchStore((state) => state.activeTabId);
  const activateTab = useWorkbenchStore((state) => state.activateTab);
  const switchProject = useWorkbenchStore((state) => state.switchProject);
  const createProject = useWorkbenchStore((state) => state.createProject);
  const addProjectRoot = useWorkbenchStore((state) => state.addProjectRoot);
  const chatTabs = tabs.filter((tab) => tab.kind === "chat");

  return (
    <aside className="project-sidebar" aria-label="Projects and chats">
      <div className="project-sidebar__actions">
        <button
          className="project-action project-action--primary"
          type="button"
          title="New chat"
        >
          <SquarePen size={16} aria-hidden="true" />
          New chat
        </button>
        <button className="project-action" type="button" title="Search chats">
          <Search size={16} aria-hidden="true" />
          Search chats
        </button>
      </div>

      <div className="project-sidebar__scroll">
        <section className="project-section">
          <div className="project-section__title">
            <span>Projects</span>
            <button
              type="button"
              aria-label="Open folder as project"
              title="Open folder as project"
              onClick={() => void createProject()}
            >
              <Plus size={14} aria-hidden="true" />
            </button>
          </div>
          {projects.map((item) => {
            const selected = item.id === project?.id;
            return (
              <div className="project-entry" key={item.id}>
                <button
                  className={`project-row ${
                    selected ? "project-row--active" : ""
                  }`}
                  type="button"
                  aria-current={selected ? "page" : undefined}
                  title={`Open project ${item.name}`}
                  onClick={() => void switchProject(item.id)}
                >
                  <FolderOpen size={16} aria-hidden="true" />
                  <span>{item.name}</span>
                  <span className="project-row__count">{item.rootCount}</span>
                  {selected ? (
                    <ChevronDown size={14} aria-hidden="true" />
                  ) : (
                    <ChevronRight size={14} aria-hidden="true" />
                  )}
                </button>
                {selected ? (
                  <div className="project-chats">
                    {chatTabs.map((tab) => (
                      <button
                        className={`project-chat ${
                          tab.id === activeTabId ? "project-chat--active" : ""
                        }`}
                        key={tab.id}
                        type="button"
                        title={`Open chat ${tab.title.replace(/^Chat:\s*/, "")}`}
                        onClick={() => void activateTab(tab.id)}
                      >
                        <MessageSquare size={14} aria-hidden="true" />
                        <span>{tab.title.replace(/^Chat:\s*/, "")}</span>
                        {tab.status === "working" ? (
                          <span
                            className="project-chat__working"
                            title="Working"
                          />
                        ) : null}
                      </button>
                    ))}
                    <button
                      className="project-chat project-chat--utility"
                      type="button"
                      title="Add folder root"
                      onClick={() => void addProjectRoot()}
                    >
                      <FolderPlus size={14} aria-hidden="true" />
                      <span>Add folder root</span>
                    </button>
                  </div>
                ) : null}
              </div>
            );
          })}
          {projects.length === 0 ? (
            <button
              className="project-empty-action"
              type="button"
              onClick={() => void createProject()}
            >
              <FolderPlus size={16} aria-hidden="true" />
              Open a folder to begin
            </button>
          ) : null}
        </section>
      </div>

      <div className="project-sidebar__account">
        <span className="account-avatar">P</span>
        <span>Username</span>
        <button type="button" aria-label="Open settings" title="Settings">
          <Settings size={15} aria-hidden="true" />
        </button>
      </div>
    </aside>
  );
}

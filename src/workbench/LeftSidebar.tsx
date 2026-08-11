import {
  ChevronDown,
  ChevronRight,
  FolderOpen,
  FolderPlus,
  Plus,
  SquarePen,
} from "lucide-react";

import { useWorkbenchStore } from "../app/store";
import { ChatSearchControl } from "../features/chatSearch/ChatSearchControl";
import { ProjectChatNavigation } from "./ProjectChatNavigation";

export function LeftSidebar() {
  const projects = useWorkbenchStore((state) => state.projects);
  const project = useWorkbenchStore((state) => state.project);
  const switchProject = useWorkbenchStore((state) => state.switchProject);
  const createProject = useWorkbenchStore((state) => state.createProject);
  const createChat = useWorkbenchStore((state) => state.createChat);

  return (
    <aside className="project-sidebar" aria-label="Projects and chats">
      <div className="project-sidebar__actions">
        <button
          className="project-action project-action--primary"
          type="button"
          title="New chat"
          disabled={!project}
          onClick={() => void createChat()}
        >
          <SquarePen size={16} aria-hidden="true" />
          New chat
        </button>
        <ChatSearchControl />
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
                {selected ? <ProjectChatNavigation /> : null}
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
    </aside>
  );
}

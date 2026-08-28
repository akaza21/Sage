import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { App } from "./app/App";
import "./styles/global.css";
import "./styles/editor-activity-bar.css";
import "./features/files/explorer-surface.css";
import "./features/search/workspace-search.css";
import "./styles/workbench-chrome.css";
import "./styles/diff-surface.css";
import "./styles/preview-surface.css";
import "./styles/file-editor-surface.css";
import "./styles/source-control-changes.css";
import "./styles/repository-sidebar-controls.css";
import "./styles/shared-dialogs.css";
import "./styles/status-surfaces.css";
import "./styles/project-navigation-surface.css";
import "./styles/chat-navigation-surface.css";
import "./styles/chat-surface.css";
import "./styles/composer-surface.css";
import "./styles/problems-outline-surfaces.css";
import "./styles/agent-rail-surface.css";

const root = document.getElementById("root");

if (!root) {
  throw new Error("Sage could not find its application root.");
}

createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>,
);

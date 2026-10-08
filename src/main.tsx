import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { App } from "./app/App";
import "./styles/global.css";
import "./styles/shared-controls.css";
import "./styles/editor-activity-bar.css";
import "./features/files/explorer-surface.css";
import "./features/search/workspace-search.css";
import "./styles/workbench-chrome.css";
import "./styles/diff-surface.css";
import "./styles/preview-surface.css";
import "./styles/file-editor-surface.css";
import "./styles/resource-document-language.css";
import "./styles/source-control-changes.css";
import "./styles/repository-sidebar-controls.css";
import "./styles/shared-dialogs.css";
import "./styles/status-surfaces.css";
import "./styles/project-navigation-surface.css";
import "./styles/chat-navigation-surface.css";
import "./styles/chat-surface.css";
import "./styles/changed-files-summary.css";
import "./styles/agent-chat-panes.css";
import "./styles/composer-surface.css";
import "./styles/composer-model-picker.css";
import "./styles/composer-image.css";
import "./styles/problems-outline-surfaces.css";
import "./styles/agent-rail-surface.css";
import "./styles/editor-visual-reference.css";
import "./styles/editor-interactions.css";
import "./styles/settings-surface.css";
import "./styles/editor-highlight-language.css";

const root = document.getElementById("root");

if (!root) {
  throw new Error("Sage could not find its application root.");
}

createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>,
);

import { mountApp } from "./app/mountApp";
import { hasWorkbenchBackend } from "./ports/workbenchRuntime";

const root = document.getElementById("root");

if (!root) {
  throw new Error("Sage could not find its application root.");
}

// Any build opened in a plain browser tab has no backend to talk to; the
// desktop shell, the dev bridge and the UI fixture all report one. The notice
// is markup in index.html, so the startup bundle never carries it.
if (hasWorkbenchBackend()) {
  mountApp(root);
} else {
  const notice = document.getElementById("backend-missing");
  if (notice instanceof HTMLTemplateElement)
    root.replaceChildren(notice.content.cloneNode(true));
}

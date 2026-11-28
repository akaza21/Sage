import { renderBackendMissing } from "./app/backendMissingScreen";
import { mountApp } from "./app/mountApp";
import { hasWorkbenchBackend } from "./ports/workbenchRuntime";

const root = document.getElementById("root");

if (!root) {
  throw new Error("Sage could not find its application root.");
}

if (import.meta.env.DEV && !hasWorkbenchBackend()) {
  renderBackendMissing(root);
} else {
  mountApp(root);
}

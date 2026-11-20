import type { CodexAuthScope } from "../domain/acp";
import { invokeCodexAuth } from "./acpHarnessDesktop";

// Status checks load with the visible auth surfaces, outside model discovery.
export const codexAuthStatusDesktop = {
  getAuth: (scope: CodexAuthScope) => invokeCodexAuth("get_codex_auth", scope),
};

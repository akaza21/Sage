import type { CodexAuthResponse, CodexAuthScope } from "../domain/acp";
import { invokeCodexAuth } from "./acpHarnessDesktop";

/** Sends the key to Codex, which stores it in its own home; Sage keeps no copy. */
export function loginCodexApiKey(
  scope: CodexAuthScope,
  apiKey: string,
): Promise<CodexAuthResponse> {
  return invokeCodexAuth("login_codex_api_key", { ...scope, apiKey });
}

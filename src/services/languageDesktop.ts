import {
  LANGUAGE_DIAGNOSTIC_EVENT,
  type LanguageDiagnosticReport,
} from "../domain/languageService";
import type { ResourceRef } from "../domain/resource";

function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

async function invokeLanguage<T>(
  command: string,
  resource: ResourceRef,
): Promise<T> {
  if (!isTauriRuntime()) {
    throw new Error(
      "Language sessions are available only in the Sage desktop runtime.",
    );
  }
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(command, { resource });
}

export const languageDesktop = {
  startLanguageSession(resource: ResourceRef): Promise<number> {
    return invokeLanguage("start_language_session", resource);
  },
  cancelLanguageSession(resource: ResourceRef): Promise<void> {
    return invokeLanguage("cancel_language_session", resource);
  },
  async subscribeDiagnostics(
    onReport: (report: LanguageDiagnosticReport) => void,
  ): Promise<() => void> {
    if (!isTauriRuntime()) {
      return () => undefined;
    }
    const { listen } = await import("@tauri-apps/api/event");
    return listen<LanguageDiagnosticReport>(
      LANGUAGE_DIAGNOSTIC_EVENT,
      (event) => {
        onReport(event.payload);
      },
    );
  },
};

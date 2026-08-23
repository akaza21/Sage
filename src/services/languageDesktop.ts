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
  document?: string,
): Promise<T> {
  if (!isTauriRuntime()) {
    throw new Error(
      "Language sessions are available only in the Sage desktop runtime.",
    );
  }
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(command, {
    resource,
    ...(document === undefined ? {} : { document }),
  });
}

export const languageDesktop = {
  startLanguageSession(
    resource: ResourceRef,
    document: string,
  ): Promise<number> {
    return invokeLanguage("start_language_session", resource, document);
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
    return listen<unknown>(LANGUAGE_DIAGNOSTIC_EVENT, (event) => {
      if (isLanguageDiagnosticReport(event.payload)) {
        onReport(event.payload);
      }
    });
  },
};

function isLanguageDiagnosticReport(
  value: unknown,
): value is LanguageDiagnosticReport {
  if (
    !isRecord(value) ||
    !isResource(value.resource) ||
    !isGeneration(value.generation)
  ) {
    return false;
  }
  return (
    Array.isArray(value.diagnostics) &&
    value.diagnostics.length <= 64 &&
    value.diagnostics.every(
      (diagnostic) =>
        isRecord(diagnostic) &&
        isResource(diagnostic.resource) &&
        typeof diagnostic.message === "string" &&
        diagnostic.message.length <= 4_096 &&
        (diagnostic.severity === "error" ||
          diagnostic.severity === "warning" ||
          diagnostic.severity === "info") &&
        isPosition(diagnostic.startLine) &&
        isPosition(diagnostic.startColumn) &&
        isPosition(diagnostic.endLine) &&
        isPosition(diagnostic.endColumn),
    )
  );
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function isResource(value: unknown): value is ResourceRef {
  return (
    isRecord(value) &&
    typeof value.rootId === "string" &&
    typeof value.relativePath === "string"
  );
}

function isGeneration(value: unknown): value is number {
  return (
    typeof value === "number" &&
    Number.isInteger(value) &&
    value > 0 &&
    value <= 0xffff_ffff
  );
}

function isPosition(value: unknown): value is number {
  return (
    typeof value === "number" &&
    Number.isInteger(value) &&
    value > 0 &&
    value <= 0xffff_ffff
  );
}

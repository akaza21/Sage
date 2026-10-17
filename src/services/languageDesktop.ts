import {
  LANGUAGE_DIAGNOSTIC_EVENT,
  LANGUAGE_OUTLINE_EVENT,
  type LanguageDiagnosticReport,
  type LanguageOutlineReport,
} from "../domain/languageService";
import type { ResourceRef } from "../domain/resource";
import { isRecord } from "../domain/guards";
import { utf8ByteLength } from "../domain/text";
import { invokeNative } from "../ports/nativeInvoke";
import { isTauriRuntime } from "../ports/desktopRuntime";

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
  return invokeNative<T>(command, {
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
  async subscribeOutline(
    onReport: (report: LanguageOutlineReport) => void,
  ): Promise<() => void> {
    if (!isTauriRuntime()) {
      return () => undefined;
    }
    const { listen } = await import("@tauri-apps/api/event");
    return listen<unknown>(LANGUAGE_OUTLINE_EVENT, (event) => {
      if (isLanguageOutlineReport(event.payload)) {
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

function isLanguageOutlineReport(
  value: unknown,
): value is LanguageOutlineReport {
  return (
    isRecord(value) &&
    isResource(value.resource) &&
    isGeneration(value.generation) &&
    Array.isArray(value.symbols) &&
    value.symbols.length <= 64 &&
    value.symbols.every(
      (symbol) =>
        isRecord(symbol) &&
        isResource(symbol.resource) &&
        typeof symbol.name === "string" &&
        utf8ByteLength(symbol.name) <= 256 &&
        (symbol.kind === "file" ||
          symbol.kind === "function" ||
          symbol.kind === "type" ||
          symbol.kind === "unknown") &&
        isPosition(symbol.startLine),
    )
  );
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

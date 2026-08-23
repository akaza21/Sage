import type { ResourceRef } from "./resource";

export const LANGUAGE_FIXTURE_ARGUMENT = "--sage-language-fixture-v1";
export const LANGUAGE_DIAGNOSTIC_EVENT = "language-diagnostic-v1";

export type LanguageDiagnosticSeverity = "error" | "warning" | "info";

export interface LanguageDiagnostic {
  resource: ResourceRef;
  message: string;
  severity: LanguageDiagnosticSeverity;
  startLine: number;
  startColumn: number;
  endLine: number;
  endColumn: number;
}

export interface LanguageDiagnosticReport {
  resource: ResourceRef;
  generation: number;
  diagnostics: readonly LanguageDiagnostic[];
}

export const LANGUAGE_OUTLINE_EVENT = "language-outline-v1";

export type LanguageOutlineKind = "file" | "function" | "type" | "unknown";

export interface LanguageOutlineSymbol {
  resource: ResourceRef;
  name: string;
  kind: LanguageOutlineKind;
  startLine: number;
}

export interface LanguageOutlineReport {
  resource: ResourceRef;
  generation: number;
  symbols: readonly LanguageOutlineSymbol[];
}

export interface LanguageSession {
  language: "typescript";
  cancel(): Promise<void>;
}

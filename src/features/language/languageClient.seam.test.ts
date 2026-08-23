import { afterEach, describe, expect, it, vi } from "vitest";

import {
  LANGUAGE_DIAGNOSTIC_EVENT,
  type LanguageDiagnosticReport,
} from "../../domain/languageService";
import { languageDesktop } from "../../services/languageDesktop";
import { startLanguageClient } from "./languageClient";

const resource = { rootId: "root-a", relativePath: "src/example.ts" };

afterEach(() => {
  vi.restoreAllMocks();
});

describe("language client seam", () => {
  it("starts through languageDesktop, bounds reports, and stops delivery after cancel", async () => {
    let deliver!: (report: LanguageDiagnosticReport) => void;
    const unsubscribe = vi.fn();
    const start = vi
      .spyOn(languageDesktop, "startLanguageSession")
      .mockResolvedValue(1);
    const cancel = vi
      .spyOn(languageDesktop, "cancelLanguageSession")
      .mockResolvedValue();
    vi.spyOn(languageDesktop, "subscribeDiagnostics").mockImplementation(
      async (onReport) => {
        deliver = onReport;
        return unsubscribe;
      },
    );
    const reports: LanguageDiagnosticReport[] = [];

    const session = await startLanguageClient(resource, "", (report) => {
      reports.push(report);
    });
    const oversized = {
      resource,
      generation: 1,
      diagnostics: Array.from({ length: 65 }, () => ({
        resource,
        message: "fixture diagnostic",
        severity: "error" as const,
        startLine: 1,
        startColumn: 1,
        endLine: 1,
        endColumn: 2,
      })),
    };

    deliver(oversized);
    expect(reports).toHaveLength(0);
    deliver({ ...oversized, diagnostics: oversized.diagnostics.slice(0, 64) });
    expect(start).toHaveBeenCalledWith(resource, "");
    expect(reports).toHaveLength(1);
    expect(reports[0]?.diagnostics).toHaveLength(64);
    expect(LANGUAGE_DIAGNOSTIC_EVENT).toBe("language-diagnostic-v1");

    await session.cancel();
    deliver({ ...oversized, diagnostics: [] });

    expect(cancel).toHaveBeenCalledWith(resource);
    expect(unsubscribe).toHaveBeenCalledOnce();
    expect(reports).toHaveLength(1);
  });
});

import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  LANGUAGE_DIAGNOSTIC_EVENT,
  type LanguageDiagnosticReport,
} from "../../domain/languageService";
import { languageDesktop } from "../../services/languageDesktop";
import { startLanguageClient, useLanguageSession } from "./languageClient";

const resource = { rootId: "root-a", relativePath: "src/example.ts" };

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((next) => {
    resolve = next;
  });
  return { promise, resolve };
}

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

  it("starts only the latest rapid revision after superseded revisions cancel", async () => {
    const first = deferred<number>();
    const start = vi
      .spyOn(languageDesktop, "startLanguageSession")
      .mockImplementationOnce(() => first.promise)
      .mockResolvedValue(2);
    const cancel = vi
      .spyOn(languageDesktop, "cancelLanguageSession")
      .mockResolvedValue();
    vi.spyOn(languageDesktop, "subscribeDiagnostics").mockResolvedValue(
      vi.fn(),
    );
    const { rerender, unmount } = renderHook(
      ({ document }) => useLanguageSession(resource, document),
      { initialProps: { document: "first" } },
    );

    await waitFor(() => expect(start).toHaveBeenCalledTimes(1));
    await act(async () => {
      rerender({ document: "superseded" });
      rerender({ document: "latest" });
    });

    expect(start).toHaveBeenCalledOnce();
    await act(async () => {
      first.resolve(1);
    });
    await waitFor(() => expect(start).toHaveBeenCalledTimes(2));
    expect(start).toHaveBeenNthCalledWith(1, resource, "first");
    expect(start).toHaveBeenNthCalledWith(2, resource, "latest");
    unmount();
    await waitFor(() => expect(cancel).toHaveBeenCalledTimes(2));
  });

  it("does not spawn after a pending client start is cancelled", async () => {
    const subscription = deferred<() => void>();
    const unsubscribe = vi.fn();
    const start = vi
      .spyOn(languageDesktop, "startLanguageSession")
      .mockResolvedValue(1);
    vi.spyOn(languageDesktop, "subscribeDiagnostics").mockReturnValue(
      subscription.promise,
    );
    const cancellation = new AbortController();
    const session = startLanguageClient(
      resource,
      "cancelled document",
      undefined,
      cancellation.signal,
    );

    cancellation.abort();
    subscription.resolve(unsubscribe);
    await session;

    expect(start).not.toHaveBeenCalled();
    expect(unsubscribe).toHaveBeenCalledOnce();
  });
});

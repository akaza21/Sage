import { useEffect, useRef } from "react";

import type {
  LanguageDiagnosticReport,
  LanguageSession,
} from "../../domain/languageService";
import type { ResourceRef } from "../../domain/resource";
import { sameResource } from "../../domain/resourceIdentity";
import { languageDesktop } from "../../services/languageDesktop";

const MAX_DIAGNOSTICS = 64;

export async function startLanguageClient(
  resource: ResourceRef,
  document: string,
  onReport: (report: LanguageDiagnosticReport) => void = () => undefined,
): Promise<LanguageSession> {
  let cancelled = false;
  const unsubscribe = await languageDesktop.subscribeDiagnostics((report) => {
    if (!cancelled && acceptsReport(report, resource)) {
      onReport(report);
    }
  });
  try {
    await languageDesktop.startLanguageSession(resource, document);
  } catch (error) {
    unsubscribe();
    throw error;
  }
  return {
    language: "typescript",
    async cancel() {
      if (cancelled) {
        return;
      }
      cancelled = true;
      unsubscribe();
      await languageDesktop.cancelLanguageSession(resource);
    },
  };
}

export function useLanguageSession(
  resource: ResourceRef | undefined,
  document: string,
): void {
  const cancellation = useRef<Promise<void>>(Promise.resolve());
  const rootId = resource?.rootId;
  const relativePath = resource?.relativePath;

  useEffect(() => {
    if (!rootId || !relativePath) {
      return;
    }
    const nextResource = { rootId, relativePath };
    let cancelled = false;
    let session: LanguageSession | undefined;
    const start = cancellation.current.then(async () => {
      const next = await startLanguageClient(nextResource, document);
      if (cancelled) {
        await next.cancel();
        return;
      }
      session = next;
    });
    void start.catch(() => undefined);

    return () => {
      cancelled = true;
      cancellation.current = start
        .catch(() => undefined)
        .then(async () => {
          await session?.cancel();
        })
        .catch(() => undefined);
    };
  }, [document, relativePath, rootId]);
}

function acceptsReport(
  report: LanguageDiagnosticReport,
  resource: ResourceRef,
): boolean {
  return (
    sameResource(report.resource, resource) &&
    report.generation > 0 &&
    report.diagnostics.length <= MAX_DIAGNOSTICS
  );
}

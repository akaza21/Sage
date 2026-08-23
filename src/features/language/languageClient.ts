import { useEffect, useRef, useState } from "react";

import type {
  LanguageDiagnosticReport,
  LanguageOutlineReport,
  LanguageSession,
} from "../../domain/languageService";
import type { ResourceRef } from "../../domain/resource";
import { sameResource } from "../../domain/resourceIdentity";
import { languageDesktop } from "../../services/languageDesktop";

const MAX_DIAGNOSTICS = 64;
const CANCELLED_LANGUAGE_SESSION: LanguageSession = {
  language: "typescript",
  async cancel() {},
};

interface ManagedLanguageSession {
  request: number;
  session: LanguageSession;
}

interface LanguageSessionRequest {
  request: number;
  resource: ResourceRef;
  document: string;
}

interface LanguageSessionLifecycle {
  nextRequest: number;
  desired?: LanguageSessionRequest;
  active?: ManagedLanguageSession;
  starting?: { request: number; cancellation: AbortController };
  runner?: Promise<void>;
}

interface LanguageProjectionDelivery {
  resource: ResourceRef;
  clear(): void;
  stop(): void;
}

const projectionDeliveries = new Set<LanguageProjectionDelivery>();

export async function startLanguageClient(
  resource: ResourceRef,
  document: string,
  onReport: (report: LanguageDiagnosticReport) => void = () => undefined,
  signal?: AbortSignal,
  onOutline: (report: LanguageOutlineReport) => void = () => undefined,
): Promise<LanguageSession> {
  let cancelled = false;
  const unsubscribeDiagnostics = await languageDesktop.subscribeDiagnostics(
    (report) => {
      if (!cancelled && acceptsReport(report, resource)) {
        onReport(report);
      }
    },
  );
  let unsubscribeOutline: () => void;
  try {
    unsubscribeOutline = await languageDesktop.subscribeOutline((report) => {
      if (!cancelled && acceptsOutlineReport(report, resource)) {
        onOutline(report);
      }
    });
  } catch (error) {
    unsubscribeDiagnostics();
    throw error;
  }
  if (signal?.aborted) {
    unsubscribeDiagnostics();
    unsubscribeOutline();
    return CANCELLED_LANGUAGE_SESSION;
  }
  try {
    await languageDesktop.startLanguageSession(resource, document);
  } catch (error) {
    unsubscribeDiagnostics();
    unsubscribeOutline();
    throw error;
  }
  return {
    language: "typescript",
    async cancel() {
      if (cancelled) {
        return;
      }
      cancelled = true;
      unsubscribeDiagnostics();
      unsubscribeOutline();
      clearProjectionReports(resource);
      await languageDesktop.cancelLanguageSession(resource);
    },
  };
}

export function useLanguageSession(
  resource: ResourceRef | undefined,
  document: string,
): void {
  const lifecycle = useRef<LanguageSessionLifecycle>({
    nextRequest: 0,
  });
  const rootId = resource?.rootId;
  const relativePath = resource?.relativePath;

  useEffect(() => {
    if (!rootId || !relativePath) {
      return;
    }
    const sessionLifecycle = lifecycle.current;
    const desired = {
      request: sessionLifecycle.nextRequest + 1,
      resource: { rootId, relativePath },
      document,
    };
    sessionLifecycle.nextRequest = desired.request;
    sessionLifecycle.desired = desired;
    runLanguageLifecycle(sessionLifecycle);

    return () => {
      if (sessionLifecycle.desired?.request === desired.request) {
        sessionLifecycle.desired = undefined;
      }
      if (sessionLifecycle.starting?.request === desired.request) {
        sessionLifecycle.starting.cancellation.abort();
      }
      runLanguageLifecycle(sessionLifecycle);
    };
  }, [document, relativePath, rootId]);
}

function runLanguageLifecycle(lifecycle: LanguageSessionLifecycle): void {
  if (lifecycle.runner) {
    return;
  }
  const runner = drainLanguageLifecycle(lifecycle);
  lifecycle.runner = runner;
  void runner.then(
    () => finishLanguageLifecycle(lifecycle, runner),
    () => finishLanguageLifecycle(lifecycle, runner),
  );
}

function finishLanguageLifecycle(
  lifecycle: LanguageSessionLifecycle,
  runner: Promise<void>,
): void {
  if (lifecycle.runner !== runner) {
    return;
  }
  lifecycle.runner = undefined;
  if (languageLifecycleNeedsWork(lifecycle)) {
    runLanguageLifecycle(lifecycle);
  }
}

function languageLifecycleNeedsWork(
  lifecycle: LanguageSessionLifecycle,
): boolean {
  return lifecycle.desired
    ? lifecycle.active?.request !== lifecycle.desired.request
    : lifecycle.active !== undefined;
}

async function drainLanguageLifecycle(
  lifecycle: LanguageSessionLifecycle,
): Promise<void> {
  while (true) {
    const desired = lifecycle.desired;
    const active = lifecycle.active;
    if (!desired) {
      if (!active) {
        return;
      }
      lifecycle.active = undefined;
      await active.session.cancel().catch(() => undefined);
      continue;
    }
    if (active?.request === desired.request) {
      return;
    }
    if (active) {
      lifecycle.active = undefined;
      await active.session.cancel().catch(() => undefined);
      continue;
    }
    const cancellation = new AbortController();
    lifecycle.starting = { request: desired.request, cancellation };
    let session: LanguageSession;
    try {
      session = await startLanguageClient(
        desired.resource,
        desired.document,
        undefined,
        cancellation.signal,
      );
    } catch {
      if (lifecycle.desired?.request === desired.request) {
        lifecycle.desired = undefined;
      }
      return;
    } finally {
      if (lifecycle.starting?.request === desired.request) {
        lifecycle.starting = undefined;
      }
    }
    if (lifecycle.desired?.request !== desired.request) {
      await session.cancel().catch(() => undefined);
      continue;
    }
    lifecycle.active = { request: desired.request, session };
  }
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

function acceptsOutlineReport(
  report: LanguageOutlineReport,
  resource: ResourceRef,
): boolean {
  return (
    sameResource(report.resource, resource) &&
    report.generation > 0 &&
    report.symbols.length <= MAX_DIAGNOSTICS
  );
}

export function useDiagnosticProjection(
  resource: ResourceRef | undefined,
): LanguageDiagnosticReport | null {
  return useLanguageProjection(
    resource,
    languageDesktop.subscribeDiagnostics,
    boundDiagnosticReport,
  );
}

export function useOutlineProjection(
  resource: ResourceRef | undefined,
): LanguageOutlineReport | null {
  return useLanguageProjection(
    resource,
    languageDesktop.subscribeOutline,
    boundOutlineReport,
  );
}

function boundDiagnosticReport(
  report: LanguageDiagnosticReport,
): LanguageDiagnosticReport {
  return {
    ...report,
    diagnostics: report.diagnostics.slice(0, MAX_DIAGNOSTICS),
  };
}

function boundOutlineReport(
  report: LanguageOutlineReport,
): LanguageOutlineReport {
  return { ...report, symbols: report.symbols.slice(0, MAX_DIAGNOSTICS) };
}

function clearProjectionReports(resource: ResourceRef): void {
  for (const delivery of [...projectionDeliveries]) {
    if (sameResource(delivery.resource, resource)) {
      delivery.clear();
    }
  }
}

function useLanguageProjection<
  T extends LanguageDiagnosticReport | LanguageOutlineReport,
>(
  resource: ResourceRef | undefined,
  subscribe: (onReport: (report: T) => void) => Promise<() => void>,
  bound: (report: T) => T,
): T | null {
  const [report, setReport] = useState<T | null>(null);
  const rootId = resource?.rootId;
  const relativePath = resource?.relativePath;
  const resourceIdentity = `${rootId ?? ""}:${relativePath ?? ""}`;
  const [seenIdentity, setSeenIdentity] = useState(resourceIdentity);
  if (seenIdentity !== resourceIdentity) {
    setSeenIdentity(resourceIdentity);
    setReport(null);
  }

  useEffect(() => {
    let sleeping = false;
    let unsubscribe: (() => void) | undefined;
    let latestGeneration = 0;
    if (!rootId || !relativePath) {
      return;
    }
    const activeResource = { rootId, relativePath };
    const delivery: LanguageProjectionDelivery = {
      resource: activeResource,
      clear() {
        if (!sleeping) {
          setReport(null);
        }
      },
      stop() {
        if (sleeping) {
          return;
        }
        delivery.clear();
        sleeping = true;
        projectionDeliveries.delete(delivery);
        unsubscribe?.();
        unsubscribe = undefined;
      },
    };
    projectionDeliveries.add(delivery);
    void subscribe((incoming) => {
      if (
        sleeping ||
        !sameResource(incoming.resource, activeResource) ||
        incoming.generation <= latestGeneration
      ) {
        return;
      }
      latestGeneration = incoming.generation;
      setReport(bound(incoming));
    }).then(
      (nextUnsubscribe) => {
        if (sleeping) {
          nextUnsubscribe();
        } else {
          unsubscribe = nextUnsubscribe;
        }
      },
      () => undefined,
    );
    return () => {
      delivery.stop();
    };
  }, [bound, relativePath, rootId, subscribe]);

  return report;
}

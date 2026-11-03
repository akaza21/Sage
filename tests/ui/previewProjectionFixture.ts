import { expect, type Page } from "@playwright/test";

type Phase = "starting" | "healthFailed" | "exited";
type Step = {
  revision: string;
  phase: string;
  failure: string | null;
  origin: string | null;
};

export async function installPreviewProjectionTransport(page: Page) {
  await page.evaluate(() => {
    // This one-visible-Chat fixture mounts two ACP consumers, the rail's
    // background status consumer and this Preview listener, and briefly
    // holds one rejected goal or session-config callback. It is a
    // fixture-local topology, never a production event cap.
    const MAX_FIXTURE_REGISTRATIONS = 5;
    const PREVIEW_EVENT = "acp-projection-changed-v1";
    const callbacks = new Map<number, (event: { payload: unknown }) => void>();
    const listeners = new Map<number, (event: { payload: unknown }) => void>();
    let next = 0;
    const release = (id: number) => {
      callbacks.delete(id);
      listeners.delete(id);
    };
    const target = {
      projectId: "visual-project",
      projectGeneration: "0",
      expectedTransitionEpoch: 0,
      chatId: "visual-chat-preview",
      sessionId: "visual-session",
      runtimeEpoch: "7",
      rootId: "visual-root",
      scriptName: "dev",
    };
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      value: {
        transformCallback(callback: (event: { payload: unknown }) => void) {
          let id = 0;
          for (
            let attempt = 0;
            attempt < MAX_FIXTURE_REGISTRATIONS;
            attempt += 1
          ) {
            next = next === MAX_FIXTURE_REGISTRATIONS ? 1 : next + 1;
            if (!callbacks.has(next)) {
              id = next;
              break;
            }
          }
          if (!id)
            throw new Error("visual callback registration limit reached");
          callbacks.set(id, callback);
          return id;
        },
        unregisterCallback: release,
        invoke: async (
          command: string,
          args: {
            event?: string;
            eventId?: number;
            handler?: number;
            request?: unknown;
          },
        ) => {
          if (command === "plugin:event|listen") {
            if (args.event !== PREVIEW_EVENT) {
              release(args.handler ?? 0);
              throw new Error("unexpected visual event subscription");
            }
            const callback = callbacks.get(args.handler ?? 0);
            if (!callback) throw new Error("visual event callback missing");
            if (listeners.size >= MAX_FIXTURE_REGISTRATIONS)
              throw new Error("visual event listener limit reached");
            const id = args.handler ?? 0;
            listeners.set(id, callback);
            return id;
          }
          if (command === "plugin:event|unlisten") {
            const id = args.eventId;
            if (
              args.event !== PREVIEW_EVENT ||
              !Number.isSafeInteger(id) ||
              callbacks.has(id) ||
              listeners.has(id)
            )
              throw new Error(
                "visual event unlisten identity was not released",
              );
            return undefined;
          }
          if (command === "discover_preview_targets")
            return {
              projectId: "visual-project",
              transitionEpoch: 0,
              targets: [target],
              truncated: false,
            };
          if (command === "get_preview_health")
            return {
              ref: args.request,
              healthy: true,
              origin: "http://127.0.0.1:4173/native",
            };
          if (command === "start_preview_target")
            return {
              ref: args.request,
              origin: "http://127.0.0.1:4173/native",
            };
          if (command === "stop_preview_target") return { ref: args.request };
          // AP-B B5: the fixture's turns made no shell edits.
          if (command === "get_acp_turn_workspace_changes")
            return {
              rootId: "visual-root",
              tracked: true,
              truncated: false,
              changes: [],
            };
          throw new Error(`unexpected visual desktop command: ${command}`);
        },
      },
    });
    Object.defineProperty(window, "__TAURI_EVENT_PLUGIN_INTERNALS__", {
      configurable: true,
      value: {
        unregisterListener: (_event: string, eventId: number) =>
          release(eventId),
      },
    });
    (
      window as Window & { __sagePreviewStep?: (step: Step) => void }
    ).__sagePreviewStep = (step) => {
      const preview = {
        projectId: "visual-project",
        projectGeneration: "0",
        transitionEpoch: 0,
        chatId: "visual-chat-preview",
        sessionId: "visual-session",
        runtimeEpoch: "7",
        target,
        previewRevision: step.revision,
        phase: step.phase,
        failure: step.failure,
        origin: step.origin,
      };
      for (const listener of listeners.values())
        listener({ payload: { kind: "preview", preview } });
    };
    (
      window as Window & {
        __sagePreviewTransportAudit?: () => Promise<boolean>;
      }
    ).__sagePreviewTransportAudit = async () => {
      const allocated = Array.from(
        { length: MAX_FIXTURE_REGISTRATIONS - callbacks.size },
        () => window.__TAURI_INTERNALS__.transformCallback(() => undefined),
      );
      let overflow = false;
      try {
        window.__TAURI_INTERNALS__.transformCallback(() => undefined);
      } catch {
        overflow = true;
      }
      const released = allocated[1];
      if (!released) return false;
      release(released);
      const reused = window.__TAURI_INTERNALS__.transformCallback(
        () => undefined,
      );
      const first = allocated[0];
      if (!first) return false;
      release(first);
      const rejected = window.__TAURI_INTERNALS__.transformCallback(
        () => undefined,
      );
      const rejectedEvent = await window.__TAURI_INTERNALS__
        .invoke("plugin:event|listen", {
          event: "unrelated-event",
          handler: rejected,
        })
        .then(() => false)
        .catch(() => true);
      const rejectedReleased = !callbacks.has(rejected);
      for (const id of [...allocated.slice(2), reused]) release(id);
      return (
        overflow && reused === released && rejectedEvent && rejectedReleased
      );
    };
  });
}

export async function assertPreviewProjectionTransportBounds(page: Page) {
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          (
            window as Window & {
              __sagePreviewTransportAudit?: () => Promise<boolean>;
            }
          ).__sagePreviewTransportAudit?.() ?? false,
      ),
    )
    .toBe(true);
}

export async function emitPreviewProjection(page: Page, terminal: Phase) {
  const activeSteps: Step[] = [
    { revision: "2", phase: "starting", failure: null, origin: null },
    { revision: "3", phase: "waitingForOrigin", failure: null, origin: null },
    { revision: "4", phase: "healthChecking", failure: null, origin: null },
    {
      revision: "5",
      phase: "active",
      failure: null,
      origin: "http://127.0.0.1:4173/native",
    },
  ];
  for (const step of activeSteps) {
    await page.evaluate(
      (value) =>
        (
          window as Window & { __sagePreviewStep?: (step: Step) => void }
        ).__sagePreviewStep?.(value),
      step,
    );
  }
  await expect(page.getByTitle("Sage browser")).toHaveAttribute(
    "src",
    "http://127.0.0.1:4173/native",
  );
  const terminalSteps: Step[] =
    terminal === "starting"
      ? [
          { revision: "6", phase: "stopping", failure: null, origin: null },
          { revision: "7", phase: "stopped", failure: null, origin: null },
          { revision: "8", phase: "starting", failure: null, origin: null },
        ]
      : terminal === "healthFailed"
        ? [
            {
              revision: "6",
              phase: "healthChecking",
              failure: null,
              origin: null,
            },
            {
              revision: "7",
              phase: "healthFailed",
              failure: "healthTimeout",
              origin: null,
            },
          ]
        : [
            {
              revision: "6",
              phase: "exited",
              failure: "exitedEarly",
              origin: null,
            },
          ];
  for (const step of terminalSteps) {
    await page.evaluate(
      (value) =>
        (
          window as Window & { __sagePreviewStep?: (step: Step) => void }
        ).__sagePreviewStep?.(value),
      step,
    );
  }
}

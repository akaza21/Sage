import type { Page } from "@playwright/test";
import type { AvailableAcpCommand } from "../../../src/services/acpCommandsDesktop";

/**
 * A desktop runtime stand-in: list_acp_commands answers with the current
 * list, and the change event reaches the listener the menu installs.
 */
export async function installNativeCommands(
  page: Page,
  commands: AvailableAcpCommand[],
) {
  await page.evaluate((initial) => {
    const callbacks = new Map<number, (event: unknown) => void>();
    const state = {
      revision: 1,
      commands: initial,
      requests: [] as unknown[],
      listeners: [] as { event: string; handler: number }[],
    };
    let nextId = 1;
    Object.assign(window, {
      __slashCommands: state,
      __TAURI_EVENT_PLUGIN_INTERNALS__: { unregisterListener: () => {} },
    });
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      value: {
        invoke: (
          command: string,
          args: { request?: unknown; event?: string; handler?: number },
        ) => {
          if (command === "list_acp_commands") {
            state.requests.push(args.request);
            return Promise.resolve({
              revision: state.revision,
              commands: state.commands,
            });
          }
          if (command === "plugin:event|listen") {
            state.listeners.push({
              event: args.event!,
              handler: args.handler!,
            });
            return Promise.resolve(state.listeners.length);
          }
          if (command === "plugin:event|unlisten") return Promise.resolve();
          return Promise.reject(new Error("visual fixture"));
        },
        transformCallback: (callback: (event: unknown) => void) => {
          callbacks.set(nextId, callback);
          return nextId++;
        },
        unregisterCallback: (id: number) => callbacks.delete(id),
      },
    });
    Object.assign(window, {
      __emitCommandsChanged: (next: typeof initial) => {
        const request = state.requests.at(-1) as {
          projectId: string;
          chatId: string;
          sessionId: string;
          runtimeEpoch: string;
        };
        state.revision += 1;
        state.commands = next;
        for (const listener of state.listeners)
          if (listener.event === "acp-commands-changed-v1")
            callbacks.get(listener.handler)?.({
              event: listener.event,
              id: listener.handler,
              payload: {
                schemaVersion: 1,
                sageProjectId: request.projectId,
                sageChatId: request.chatId,
                sageAgentSessionId: request.sessionId,
                runtimeEpoch: request.runtimeEpoch,
                revision: state.revision,
              },
            });
      },
    });
  }, commands);
}

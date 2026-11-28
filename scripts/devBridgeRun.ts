import { readFile } from "node:fs/promises";
import type { IncomingMessage, ServerResponse } from "node:http";
import type { Plugin } from "vite";

/** Same-origin path the dev tab reads its bridge run from. */
export const BRIDGE_RUN_PATH = "/__sage/bridge-run";
const MAX_RECORD_BYTES = 512;

type BridgeRun = { run: string; bridgePort: number; code?: string };

function parseRun(text: string): BridgeRun | undefined {
  let value: unknown;
  try {
    value = JSON.parse(text);
  } catch {
    return undefined;
  }
  if (!value || typeof value !== "object") return undefined;
  const { run, bridgePort, code } = value as Record<string, unknown>;
  if (typeof run !== "string" || !/^[a-f0-9]{32}$/.test(run)) return undefined;
  if (
    typeof bridgePort !== "number" ||
    !Number.isInteger(bridgePort) ||
    bridgePort < 1 ||
    bridgePort > 65535
  )
    return undefined;
  if (
    code !== undefined &&
    (typeof code !== "string" || !/^[a-f0-9]{64}$/.test(code))
  )
    return undefined;
  return code === undefined ? { run, bridgePort } : { run, bridgePort, code };
}

function reply(response: ServerResponse, status: number, body: string) {
  // No CORS headers: another origin can never read a run, even by mistake.
  response.statusCode = status;
  response.setHeader("Cache-Control", "no-store");
  response.setHeader("Content-Type", "application/json");
  response.end(body);
}

/**
 * Serves the debug bridge's run record (port and single-use pairing code,
 * written by `src-tauri/src/dev_bridge/run_record.rs` into the disposable
 * profile) to same-origin page scripts only. 404: this dev server has no
 * backend; 503: the backend has not published a run yet.
 */
export function bridgeRunHandler(runFile: string | undefined) {
  return async (
    request: IncomingMessage,
    response: ServerResponse,
    next: () => void,
  ) => {
    if (request.url?.split("?")[0] !== BRIDGE_RUN_PATH) {
      next();
      return;
    }
    if (request.method !== "GET") {
      reply(response, 405, "{}");
      return;
    }
    if (request.headers["sec-fetch-site"] !== "same-origin") {
      reply(response, 403, "{}");
      return;
    }
    if (!runFile) {
      reply(response, 404, "{}");
      return;
    }
    let text: string;
    try {
      text = await readFile(runFile, "utf8");
    } catch {
      reply(response, 503, "{}");
      return;
    }
    const run = text.length <= MAX_RECORD_BYTES ? parseRun(text) : undefined;
    if (run) reply(response, 200, JSON.stringify(run));
    else reply(response, 503, "{}");
  };
}

/** Dev server only (`apply: "serve"`); builds never carry the route. */
export function devBridgeRunPlugin(
  runFile = process.env.SAGE_DEV_BRIDGE_RUN_FILE,
): Plugin {
  return {
    name: "sage-dev-bridge-run",
    apply: "serve",
    configureServer(server) {
      const handle = bridgeRunHandler(runFile);
      server.middlewares.use((request, response, next) => {
        void handle(request, response, next);
      });
    },
  };
}

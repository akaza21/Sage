// A debug backend started by `npm run dev` publishes each run (bridge port and
// single-use pairing code) and the dev server serves it same-origin
// (scripts/devBridgeRun.ts). The tab pairs from it without a copied link, and
// after a backend restart waits here for the next run.
const BRIDGE_RUN_PATH = "/__sage/bridge-run";
const FIRST_DELAY_MS = 250;
const MAX_DELAY_MS = 2_000;
// A cold debug build can take minutes; past this the tab stops polling.
const WAIT_LIMIT_MS = 15 * 60_000;
// A dev server that accepts the request but never answers must not stall polling.
const FETCH_LIMIT_MS = 3_000;

export type BridgeRun = { run: string; bridgePort: number; code?: string };

function parseRun(value: unknown): BridgeRun | undefined {
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
  if (typeof code === "string" && /^[a-f0-9]{64}$/.test(code))
    return { run, bridgePort, code };
  return { run, bridgePort };
}

/** The current run, or undefined while the backend has none published. */
async function readBridgeRun(): Promise<BridgeRun | "no-backend" | undefined> {
  const abort = new AbortController();
  const timer = setTimeout(() => abort.abort(), FETCH_LIMIT_MS);
  try {
    const response = await fetch(BRIDGE_RUN_PATH, {
      cache: "no-store",
      credentials: "same-origin",
      signal: abort.signal,
    });
    if (response.status === 404) return "no-backend";
    if (!response.ok) return undefined;
    return parseRun(await response.json());
  } catch {
    return undefined;
  } finally {
    clearTimeout(timer);
  }
}

/**
 * Polls the published run with backoff until `step` settles on a value, up to
 * an absolute deadline. `step` may try to connect; undefined means keep going.
 */
export async function pollBridgeRuns<T>(
  step: (run: BridgeRun) => Promise<T | undefined> | T | undefined,
): Promise<T> {
  const deadline = Date.now() + WAIT_LIMIT_MS;
  for (
    let delay = FIRST_DELAY_MS;
    ;
    delay = Math.min(delay * 2, MAX_DELAY_MS)
  ) {
    const run = await readBridgeRun();
    if (run === "no-backend")
      throw new Error("Start Sage with npm run dev to use this page.");
    const settled = run && (await step(run));
    if (settled !== undefined) return settled;
    if (Date.now() + delay > deadline)
      throw new Error(
        "The Sage backend did not start. Reload this tab to retry.",
      );
    await new Promise((wait) => setTimeout(wait, delay));
  }
}

/** One status line at the bottom of the tab; returns its remover. */
export function showBridgeNotice(text: string, alert = false): () => void {
  const notice = document.createElement("div");
  notice.setAttribute("role", alert ? "alert" : "status");
  notice.textContent = text;
  notice.style.cssText = `position: fixed; inset: auto 0 0; z-index: 2147483647; padding: 8px 16px; background: ${alert ? "#5a1d1d" : "#2b3a2b"}; color: #fff; font: 13px system-ui`;
  document.body.append(notice);
  return () => notice.remove();
}

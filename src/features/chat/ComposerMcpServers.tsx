import { useCallback, useEffect, useRef, useState } from "react";
import {
  mcpReconnectOffered,
  mcpSignInOffered,
  type McpConnection,
  type McpInventory,
  type McpServer,
} from "../../domain/acpMcp";
import type { AcpUsageRequest } from "../../domain/acpUsage";
import { errorMessage } from "../../domain/errors";
import { acpMcpDesktop } from "../../services/acpMcpDesktop";

const CONNECTION: Record<McpConnection, string> = {
  notStarted: "Not started",
  starting: "Starting",
  connected: "Connected",
  signInRequired: "Sign-in required",
  failed: "Failed",
  cancelled: "Cancelled",
  disabled: "Disabled",
  unknown: "Status unknown",
};

const plural = (count: number, one: string) =>
  `${count.toLocaleString()} ${one}${count === 1 ? "" : "s"}`;

type McpLoad =
  | { kind: "loading" }
  | { kind: "ready"; inventory: McpInventory }
  | { kind: "error" };

export function ComposerMcpServers({
  request,
}: {
  request: AcpUsageRequest | null;
}) {
  const [load, setLoad] = useState<McpLoad>({ kind: "loading" });
  // The one call running: a read, the reload, or a server's sign-in. The
  // session takes one at a time, so nothing else starts meanwhile.
  const [busy, setBusy] = useState<string | null>("read");
  const busyRef = useRef<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const start = useCallback(
    (
      what: string,
      call: (request: AcpUsageRequest) => Promise<McpInventory | void>,
      done?: () => void,
    ) => {
      if (!request || busyRef.current) return;
      busyRef.current = what;
      setBusy(what);
      setNotice(null);
      void call(request)
        .then(
          (inventory) => {
            if (inventory) setLoad({ kind: "ready", inventory });
            done?.();
          },
          (error: unknown) => {
            const message = errorMessage(error);
            // A list already shown stays; the failure is a notice.
            setLoad((shown) =>
              shown.kind === "ready" ? shown : { kind: "error" },
            );
            setNotice(message);
          },
        )
        .finally(() => {
          busyRef.current = null;
          setBusy(null);
        });
    },
    [request],
  );
  const read = useCallback(() => start("read", acpMcpDesktop.read), [start]);
  useEffect(() => {
    read();
    // A sign-in finishes in the browser; coming back shows its result.
    window.addEventListener("focus", read);
    return () => window.removeEventListener("focus", read);
  }, [read]);
  if (!request)
    return <p role="status">MCP servers appear once the session is running.</p>;
  const actions = load.kind === "ready" ? load.inventory.actions : null;
  const reload = () => start("reload", acpMcpDesktop.reload);
  const signIn = (server: McpServer) =>
    start(
      `signIn:${server.name}`,
      (live) => acpMcpDesktop.signIn({ ...live, name: server.name }),
      () =>
        setNotice(
          `Finish signing in to ${server.title ?? server.name} in your browser.`,
        ),
    );
  const reconnect = (server: McpServer) =>
    start(`reconnect:${server.name}`, (live) =>
      acpMcpDesktop.reconnect({ ...live, name: server.name }),
    );
  const serverAction = (
    server: McpServer,
    what: "signIn" | "reconnect",
    label: string,
    pending: string,
  ) => (
    <button
      type="button"
      className="button button--compact button--primary"
      aria-disabled={busy !== null}
      aria-label={`${what === "signIn" ? "Sign in to" : label} ${server.title ?? server.name}`}
      onClick={() => (what === "signIn" ? signIn : reconnect)(server)}
    >
      {busy === `${what}:${server.name}` ? pending : label}
    </button>
  );
  return (
    <div className="composer-command-panel__mcp">
      <div className="composer-command-panel__heading">
        <strong>MCP servers</strong>
        {actions?.reload ? (
          <button
            type="button"
            className="button button--compact"
            aria-disabled={busy !== null}
            onClick={reload}
          >
            {busy === "reload" ? "Reloading…" : "Reload"}
          </button>
        ) : load.kind === "error" ? (
          // No list arrived, so there is nothing to reload or reconnect yet.
          <button
            type="button"
            className="button button--compact"
            aria-disabled={busy !== null}
            onClick={read}
          >
            {busy === "read" ? "Reading…" : "Try again"}
          </button>
        ) : null}
      </div>
      {notice ? (
        <p role={load.kind === "error" ? "alert" : "status"}>{notice}</p>
      ) : null}
      {load.kind === "loading" ? (
        <p role="status">Loading MCP servers…</p>
      ) : load.kind === "error" ? null : load.inventory.servers.length === 0 ? (
        <p role="status">This session has no MCP servers.</p>
      ) : (
        <ul className="composer-command-panel__servers">
          {load.inventory.servers.map((server) => (
            <li key={server.name}>
              <span className="composer-command-panel__server">
                <strong>{server.title ?? server.name}</strong>
                <span
                  className={`composer-command-panel__state composer-command-panel__state--${server.connection}`}
                >
                  {CONNECTION[server.connection]}
                </span>
              </span>
              <span className="composer-command-panel__detail">
                {[
                  server.title ? server.name : null,
                  plural(server.tools, "tool"),
                  server.resources
                    ? plural(server.resources, "resource")
                    : null,
                  server.origin,
                ]
                  .filter(Boolean)
                  .join(" · ")}
              </span>
              {server.error ? (
                <span className="composer-command-panel__error">
                  {server.error}
                </span>
              ) : null}
              {mcpSignInOffered(server) ? (
                actions?.signIn ? (
                  serverAction(server, "signIn", "Sign in", "Opening…")
                ) : (
                  <span className="composer-command-panel__detail">
                    Sign in to this server from a terminal.
                  </span>
                )
              ) : actions?.reconnect && mcpReconnectOffered(server) ? (
                serverAction(server, "reconnect", "Reconnect", "Reconnecting…")
              ) : null}
            </li>
          ))}
        </ul>
      )}
      {load.kind === "ready" && load.inventory.truncated ? (
        <p>Only the first {load.inventory.servers.length} servers are shown.</p>
      ) : null}
    </div>
  );
}

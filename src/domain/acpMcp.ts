import type { AcpUsageRequest } from "./acpUsage";

/** Whether the session reached an MCP server. */
export const mcpConnections = [
  "notStarted",
  "starting",
  "connected",
  "signInRequired",
  "failed",
  "cancelled",
  "disabled",
  "unknown",
] as const;
export type McpConnection = (typeof mcpConnections)[number];

/** How an MCP server authenticates. */
export const mcpAuths = [
  "oAuth",
  "token",
  "signedOut",
  "none",
  "unknown",
] as const;
export type McpAuth = (typeof mcpAuths)[number];

/** One MCP server as the session's harness reports it. */
export interface McpServer {
  name: string;
  title: string | null;
  version: string | null;
  connection: McpConnection;
  auth: McpAuth;
  tools: number;
  resources: number;
  /** Why its tools could not be listed. */
  error: string | null;
  /** The scheme, host and port of an HTTP server. */
  origin: string | null;
}

/** What the user can ask of the servers through this harness. */
export interface McpActions {
  /** Read the whole MCP configuration again. */
  reload: boolean;
  /** Open a server's sign-in page, which Sage can do on this system. */
  signIn: boolean;
  /** Connect one server again. */
  reconnect: boolean;
}

export interface McpInventory {
  servers: McpServer[];
  /** The harness listed more servers than are shown. */
  truncated: boolean;
  actions: McpActions;
}

/** One live runtime's server, by name. */
export interface McpServerRequest extends AcpUsageRequest {
  name: string;
}

/** Whether the server waits on the user signing in. */
export const mcpSignInOffered = (server: McpServer) =>
  server.auth === "signedOut" || server.connection === "signInRequired";

/** Whether connecting the server again could change its state. */
export const mcpReconnectOffered = (server: McpServer) =>
  server.connection === "failed" ||
  server.connection === "cancelled" ||
  server.connection === "unknown";

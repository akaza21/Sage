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

export interface McpInventory {
  servers: McpServer[];
  /** The harness listed more servers than are shown. */
  truncated: boolean;
  /** Sage can open a server's sign-in page on this system. */
  signInAvailable: boolean;
}

/** One live runtime's server, by name. */
export interface McpSignInRequest extends AcpUsageRequest {
  name: string;
}

/** Whether the user can start this server's sign-in. */
export const mcpSignInOffered = (server: McpServer) =>
  server.auth === "signedOut" || server.connection === "signInRequired";

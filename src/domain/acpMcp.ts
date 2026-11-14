import type { AcpUsageRequest } from "./acpUsage";

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

export const mcpAuths = [
  "oAuth",
  "token",
  "signedOut",
  "none",
  "unknown",
] as const;
export type McpAuth = (typeof mcpAuths)[number];

export interface McpServer {
  name: string;
  title: string | null;
  version: string | null;
  connection: McpConnection;
  auth: McpAuth;
  tools: number;
  resources: number;
  error: string | null;
  /** The scheme, host and port of an HTTP server. */
  origin: string | null;
}

export interface McpActions {
  reload: boolean;
  signIn: boolean;
  reconnect: boolean;
}

export interface McpInventory {
  servers: McpServer[];
  truncated: boolean;
  actions: McpActions;
}

export interface McpServerRequest extends AcpUsageRequest {
  name: string;
}

export const mcpSignInOffered = (server: McpServer) =>
  server.auth === "signedOut" || server.connection === "signInRequired";

export const mcpReconnectOffered = (server: McpServer) =>
  server.connection === "failed" ||
  server.connection === "cancelled" ||
  server.connection === "unknown";

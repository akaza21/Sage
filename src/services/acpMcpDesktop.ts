import {
  mcpAuths,
  mcpConnections,
  type McpInventory,
  type McpServer,
  type McpSignInRequest,
} from "../domain/acpMcp";
import type { AcpUsageRequest } from "../domain/acpUsage";
import { AcpDesktopError, invokeAcp } from "./acpDesktopCommand";
import { isOneOf, isRecord, safeText } from "./acpDesktopGuards";

// The native inventory's bounds.
const MAX_SERVERS = 64;
const MAX_COUNT = 0xffff_ffff;

const text = (value: unknown, max: number): value is string =>
  typeof value === "string" &&
  value.length > 0 &&
  !/\p{Cc}/u.test(value) &&
  safeText(value, max) !== null;

const nullableText = (value: unknown, max: number) =>
  value === null || text(value, max);

const count = (value: unknown): value is number =>
  Number.isSafeInteger(value) &&
  (value as number) >= 0 &&
  (value as number) <= MAX_COUNT;

function decodeServer(value: unknown): McpServer | null {
  if (
    !isRecord(value) ||
    !text(value.name, 128) ||
    !nullableText(value.title, 256) ||
    !nullableText(value.version, 128) ||
    !isOneOf(value.connection, mcpConnections) ||
    !isOneOf(value.auth, mcpAuths) ||
    !count(value.tools) ||
    !count(value.resources) ||
    !nullableText(value.error, 512) ||
    !(
      value.origin === null ||
      (text(value.origin, 256) && /^https?:\/\//u.test(value.origin))
    )
  )
    return null;
  return {
    name: value.name,
    title: value.title as string | null,
    version: value.version as string | null,
    connection: value.connection,
    auth: value.auth,
    tools: value.tools,
    resources: value.resources,
    error: value.error as string | null,
    origin: value.origin as string | null,
  };
}

export function decodeMcpInventory(value: unknown): McpInventory | null {
  if (
    !isRecord(value) ||
    typeof value.truncated !== "boolean" ||
    !Array.isArray(value.servers) ||
    value.servers.length > MAX_SERVERS
  )
    return null;
  const servers: McpServer[] = [];
  for (const candidate of value.servers) {
    const server = decodeServer(candidate);
    if (!server) return null;
    servers.push(server);
  }
  return { servers, truncated: value.truncated };
}

function inventory(value: unknown): McpInventory {
  const decoded = decodeMcpInventory(value);
  if (!decoded)
    throw new AcpDesktopError({
      kind: "internal",
      message: "The MCP server list was not a valid bounded response.",
    });
  return decoded;
}

/** `/mcp`: loaded only with its panel. */
export const acpMcpDesktop = {
  async read(request: AcpUsageRequest): Promise<McpInventory> {
    return inventory(await invokeAcp("read_acp_mcp_servers", request));
  },
  async reload(request: AcpUsageRequest): Promise<McpInventory> {
    return inventory(await invokeAcp("reload_acp_mcp_servers", request));
  },
  /** Opens the server's sign-in page in the user's browser. */
  async signIn(request: McpSignInRequest): Promise<void> {
    await invokeAcp("sign_in_acp_mcp_server", request);
  },
};

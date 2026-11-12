import { describe, expect, it } from "vitest";
import { decodeMcpInventory } from "./acpMcpDesktop";

const codex = { reload: true, signIn: true, reconnect: false };
const claude = { reload: false, signIn: false, reconnect: true };

const server = {
  name: "notion",
  title: "Notion",
  version: "1.2.0",
  connection: "signInRequired",
  auth: "signedOut",
  tools: 0,
  resources: 0,
  error: null,
  origin: "https://mcp.notion.com",
};

describe("native MCP server list contract", () => {
  it("admits the bounded inventory as the native side sends it", () => {
    const value = {
      servers: [server],
      truncated: false,
      actions: codex,
    };
    expect(decodeMcpInventory(value)).toEqual(value);
    expect(
      decodeMcpInventory({
        servers: [],
        truncated: true,
        actions: claude,
      }),
    ).toEqual({
      servers: [],
      truncated: true,
      actions: claude,
    });
  });

  it("rejects unknown states, unbounded text, bad counts and odd origins", () => {
    for (const changed of [
      { name: "" },
      { name: "a\nb" },
      { title: "x".repeat(257) },
      { connection: "rebooting" },
      { auth: "password" },
      { tools: -1 },
      { tools: 1.5 },
      { resources: 2 ** 33 },
      { error: "e".repeat(513) },
      { origin: "file:///etc/passwd" },
      { origin: "javascript:alert(1)" },
    ])
      expect(
        decodeMcpInventory({
          servers: [{ ...server, ...changed }],
          truncated: false,
          actions: codex,
        }),
      ).toBeNull();
    expect(
      decodeMcpInventory({
        servers: Array(65).fill(server),
        truncated: true,
        actions: claude,
      }),
    ).toBeNull();
    expect(decodeMcpInventory({ servers: [server] })).toBeNull();
    expect(
      decodeMcpInventory({ servers: [server], truncated: false }),
    ).toBeNull();
    expect(
      decodeMcpInventory({
        servers: [server],
        truncated: false,
        actions: { reload: true, signIn: "yes", reconnect: false },
      }),
    ).toBeNull();
  });
});

// @vitest-environment node
import { existsSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { request } from "node:http";
import type { AddressInfo } from "node:net";
import path from "node:path";
import { createServer, type ViteDevServer } from "vite";
import { afterAll, beforeAll, expect, it } from "vitest";

// The real dev server config. An old throwaway profile left in a checkout
// (.sage-dev/) held the pairing code and sage.db inside the server's root.
const probeDirectory = path.resolve(".sage-dev");
const probe = path.join(probeDirectory, `exposure-probe-${process.pid}.json`);
const createdDirectory = !existsSync(probeDirectory);
let server: ViteDevServer;
let port: number;

beforeAll(async () => {
  mkdirSync(probeDirectory, { recursive: true });
  writeFileSync(probe, '{"code":"secret"}');
  server = await createServer({
    configFile: path.resolve("vite.config.ts"),
    logLevel: "silent",
    server: { port: 0, strictPort: false, hmr: false, watch: null },
  });
  await server.listen();
  port = (server.httpServer!.address() as AddressInfo).port;
}, 60_000);

afterAll(async () => {
  await server?.close();
  if (createdDirectory)
    rmSync(probeDirectory, { recursive: true, force: true });
  else rmSync(probe, { force: true });
});

function get(route: string, headers: Record<string, string> = {}) {
  return new Promise<{ status: number; body: string; cors?: unknown }>(
    (resolve, reject) => {
      const outgoing = request(
        { host: "127.0.0.1", port, path: route, headers },
        (response) => {
          let body = "";
          response.setEncoding("utf8");
          response.on("data", (chunk: string) => (body += chunk));
          response.on("end", () =>
            resolve({
              status: response.statusCode ?? 0,
              body,
              cors: response.headers["access-control-allow-origin"],
            }),
          );
        },
      );
      outgoing.on("error", reject);
      outgoing.end();
    },
  );
}

it("never serves a throwaway profile left in the checkout, raw or transformed", async () => {
  const route = `/.sage-dev/${path.basename(probe)}`;
  for (const suffix of ["", "?import", "?raw", "?url"]) {
    const response = await get(`${route}${suffix}`, {
      Origin: "http://localhost:3000",
    });
    expect(response.status, suffix).toBe(403);
    expect(response.body).not.toContain("secret");
  }
});

it("gives another localhost origin no CORS grant on the app's modules", async () => {
  const response = await get("/src/main.tsx", {
    Origin: "http://localhost:3000",
  });
  expect(response.status).toBe(200);
  expect(response.cors).toBeUndefined();
});

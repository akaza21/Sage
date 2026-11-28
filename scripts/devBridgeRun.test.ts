// @vitest-environment node
import { spawn } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { createServer, request, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import { tmpdir } from "node:os";
import path from "node:path";
import { afterAll, afterEach, expect, it } from "vitest";
import {
  BRIDGE_PAGE_PATH,
  BRIDGE_RUN_PATH,
  bridgeRunHandler,
} from "./devBridgeRun";

const servers: Server[] = [];
const directory = mkdtempSync(path.join(tmpdir(), "sage-bridge-run-"));
const runFile = path.join(directory, "dev-bridge-run.json");
const run = {
  run: "1".repeat(32),
  bridgePort: 40001,
  code: "c".repeat(64),
};

afterAll(() => rmSync(directory, { recursive: true, force: true }));
afterEach(() => {
  for (const server of servers.splice(0)) server.close();
  rmSync(runFile, { force: true });
});

async function serve(file: string | undefined) {
  const handle = bridgeRunHandler(file);
  const server = createServer((incoming, response) => {
    void handle(incoming, response, () => {
      response.statusCode = 418;
      response.end();
    });
  });
  servers.push(server);
  await new Promise<void>((ready) => server.listen(0, "127.0.0.1", ready));
  return (server.address() as AddressInfo).port;
}

function get(
  port: number,
  headers: Record<string, string>,
  route = BRIDGE_RUN_PATH,
) {
  return new Promise<{
    status: number;
    headers: Record<string, unknown>;
    body: string;
  }>((resolve, reject) => {
    const outgoing = request(
      { host: "127.0.0.1", port, path: route, headers },
      (response) => {
        let body = "";
        response.setEncoding("utf8");
        response.on("data", (chunk: string) => (body += chunk));
        response.on("end", () =>
          resolve({
            status: response.statusCode ?? 0,
            headers: response.headers,
            body,
          }),
        );
      },
    );
    outgoing.on("error", reject);
    outgoing.end();
  });
}

it("hands the run only to same-origin page scripts, never with CORS headers", async () => {
  writeFileSync(runFile, JSON.stringify(run));
  const port = await serve(runFile);

  for (const site of ["cross-site", "same-site", "none"]) {
    const refused = await get(port, {
      "Sec-Fetch-Site": site,
      Origin: "http://evil.test",
    });
    expect(refused.status).toBe(403);
    expect(refused.body).not.toContain(run.code);
  }
  const unlabelled = await get(port, {});
  expect(unlabelled.status).toBe(403);

  const granted = await get(port, { "Sec-Fetch-Site": "same-origin" });
  expect(granted.status).toBe(200);
  expect(JSON.parse(granted.body)).toEqual(run);
  expect(granted.headers["cache-control"]).toBe("no-store");
  expect(
    Object.keys(granted.headers).filter((name) =>
      name.startsWith("access-control-"),
    ),
  ).toEqual([]);
  expect((await get(port, {}, "/src/main.tsx")).status).toBe(418);
});

it("reports a missing backend apart from one still starting", async () => {
  const sameOrigin = { "Sec-Fetch-Site": "same-origin" };
  expect((await get(await serve(undefined), sameOrigin)).status).toBe(404);
  const port = await serve(runFile);
  expect((await get(port, sameOrigin)).status).toBe(503);
  writeFileSync(runFile, '{"run":"not hex","bridgePort":1}');
  expect((await get(port, sameOrigin)).status).toBe(503);
  writeFileSync(runFile, JSON.stringify({ run: run.run, bridgePort: 40001 }));
  expect(JSON.parse((await get(port, sameOrigin)).body)).toEqual({
    run: run.run,
    bridgePort: 40001,
  });
});

it("sends the app root to the paired page only when a live backend is configured", async () => {
  const live = await serve(runFile);
  for (const route of ["/", "/index.html", "/?bridge=1"]) {
    const response = await get(live, {}, route);
    expect(response.status).toBe(302);
    expect(response.headers.location).toBe(
      route.includes("?") ? `${BRIDGE_PAGE_PATH}?bridge=1` : BRIDGE_PAGE_PATH,
    );
  }
  expect((await get(live, {}, "/src/main.tsx")).status).toBe(418);

  const fixture = await serve(undefined);
  expect((await get(fixture, {}, "/")).status).toBe(418);
});

it("moves a localhost tab to the 127.0.0.1 origin the bridge admits", async () => {
  const live = await serve(runFile);
  const response = await get(live, { host: "localhost:1461" }, "/");
  expect(response.status).toBe(302);
  expect(response.headers.location).toBe(
    `http://127.0.0.1:1461${BRIDGE_PAGE_PATH}`,
  );
});

it("refuses a record over 512 bytes, counted in bytes rather than characters", async () => {
  const sameOrigin = { "Sec-Fetch-Site": "same-origin" };
  const port = await serve(runFile);
  const sized = (pad: string, bytes: number) => {
    const body = JSON.stringify({ ...run, pad: "" });
    const room = bytes - Buffer.byteLength(body);
    return JSON.stringify({
      ...run,
      pad: pad.repeat(room / Buffer.byteLength(pad)),
    });
  };
  writeFileSync(runFile, sized("x", 512));
  expect((await get(port, sameOrigin)).status).toBe(200);
  // Two-byte characters: fewer than 512 characters, but 514 bytes.
  const wide = sized("é", 512 + 2);
  expect(wide.length).toBeLessThan(512);
  writeFileSync(runFile, wide);
  expect((await get(port, sameOrigin)).status).toBe(503);
});

it("hides a run whose backend process has exited, as during a Rust rebuild", async () => {
  const sameOrigin = { "Sec-Fetch-Site": "same-origin" };
  const port = await serve(runFile);
  writeFileSync(runFile, JSON.stringify({ ...run, pid: process.pid }));
  expect(JSON.parse((await get(port, sameOrigin)).body)).toEqual(run);

  const exited = spawn(process.execPath, ["-e", ""]);
  await new Promise((done) => exited.once("exit", done));
  writeFileSync(runFile, JSON.stringify({ ...run, pid: exited.pid }));
  expect((await get(port, sameOrigin)).status).toBe(503);
});

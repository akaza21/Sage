import { createHash, randomBytes } from "node:crypto";
import { existsSync, lstatSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

export const AV01_PREVIEW_CASES = Object.freeze([
  "starting-active",
  "health-failed",
  "exited",
]);

const MAX_FIXTURE_BYTES = 64 * 1024;
const SHA256 = /^[a-f0-9]{64}$/u;

function fail(code) {
  throw new Error(`av01-preview-fixture-${code}`);
}

function absolute(value) {
  return (
    typeof value === "string" &&
    (path.isAbsolute(value) || path.win32.isAbsolute(value))
  );
}

function exactKeys(value, keys) {
  return (
    value &&
    typeof value === "object" &&
    !Array.isArray(value) &&
    Object.keys(value).sort().join("\0") === [...keys].sort().join("\0")
  );
}

function caseName(value) {
  if (!AV01_PREVIEW_CASES.includes(value)) fail("case-invalid");
  return value;
}

function fileIdentity(filePath) {
  const item = lstatSync(filePath);
  if (item.isSymbolicLink() || !item.isFile() || item.size > MAX_FIXTURE_BYTES)
    fail("file-invalid");
  return Object.freeze({
    name: path.basename(filePath),
    bytes: item.size,
    sha256: createHash("sha256").update(readFileSync(filePath)).digest("hex"),
  });
}

function assertPlainFixtureAncestors(fixture) {
  let current = path.resolve(fixture);
  while (true) {
    const item = lstatSync(current);
    if (item.isSymbolicLink() || !item.isDirectory()) fail("fixture-invalid");
    const parent = path.dirname(current);
    if (parent === current) return;
    current = parent;
  }
}

function fixtureScript() {
  return `import { createServer } from "node:http";
import { createServer as createNetServer } from "node:net";
import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";

const root = process.cwd();
const descriptor = JSON.parse(readFileSync(path.join(root, "av01-preview-case.json"), "utf8"));
const startup = path.join(root, "av01-preview-startup.json");
writeFileSync(startup, JSON.stringify({ schemaVersion: 1, processId: process.pid, nonce: descriptor.nonce, case: descriptor.case, startedAt: new Date().toISOString() }) + "\\n", { flag: "wx" });
if (descriptor.case === "exited") process.exit(0);
if (descriptor.case === "health-failed") {
  const manual = createServer((_request, response) => {
    response.writeHead(200, { "content-type": "text/html; charset=utf-8" });
    response.end("<!doctype html><main>AV01_MANUAL_" + descriptor.nonce + "</main>");
  });
  const sockets = new Set();
  const server = createNetServer(socket => {
    sockets.add(socket);
    socket.once("close", () => sockets.delete(socket));
    if (sockets.size > 8) socket.destroy();
    else socket.end("AV01_NON_HTTP_HEALTH_FAILURE\\r\\n");
  });
  manual.listen(0, "127.0.0.1", () => server.listen(0, "127.0.0.1", () => {
    const address = server.address(), manualAddress = manual.address();
    if (!address || typeof address === "string" || !manualAddress || typeof manualAddress === "string") process.exitCode = 1;
    else {
      writeFileSync(path.join(root, "av01-preview-manual.json"), JSON.stringify({ schemaVersion: 1, nonce: descriptor.nonce, origin: "http://127.0.0.1:" + manualAddress.port + "/" }) + "\\n", { flag: "wx" });
      console.log("http://127.0.0.1:" + address.port + "/");
    }
  }));
} else {
  setTimeout(() => {
    const server = createServer((_request, response) => {
      response.writeHead(200, { "content-type": "text/html; charset=utf-8" });
      response.end("<!doctype html><title>AV01 fixture</title><main>AV01_PREVIEW_FIXTURE_" + descriptor.nonce + "</main>");
    });
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      if (!address || typeof address === "string") process.exitCode = 1;
      else console.log("http://127.0.0.1:" + address.port + "/");
    });
  }, 1_000);
}
`;
}

export function createAv01PreviewFixture({ fixture, previewCase, nonce } = {}) {
  if (!absolute(fixture) || !existsSync(fixture)) fail("fixture-invalid");
  if (lstatSync(fixture).isSymbolicLink() || !lstatSync(fixture).isDirectory())
    fail("fixture-invalid");
  assertPlainFixtureAncestors(fixture);
  const selected = caseName(previewCase);
  const fixtureNonce =
    nonce ?? createHash("sha256").update(randomBytes(32)).digest("hex");
  if (!SHA256.test(fixtureNonce)) fail("nonce-invalid");
  const files = {
    "package.json": `${JSON.stringify(
      {
        private: true,
        name: "sage-av01-preview-fixture",
        version: "1.0.0",
        scripts: { dev: "node preview-fixture.mjs" },
      },
      null,
      2,
    )}\n`,
    "av01-preview-case.json": `${JSON.stringify(
      { schemaVersion: 1, case: selected, nonce: fixtureNonce },
      null,
      2,
    )}\n`,
    "preview-fixture.mjs": fixtureScript(),
  };
  for (const [name, contents] of Object.entries(files)) {
    writeFileSync(path.join(fixture, name), contents, { flag: "wx" });
  }
  const identity = Object.freeze({
    schemaVersion: 1,
    case: selected,
    nonce: fixtureNonce,
    files: Object.freeze(
      Object.keys(files)
        .sort()
        .map((name) => fileIdentity(path.join(fixture, name))),
    ),
  });
  writeFileSync(
    path.join(fixture, "fixture-identity.json"),
    `${JSON.stringify(identity, null, 2)}\n`,
    { flag: "wx" },
  );
  return identity;
}

export function readAv01PreviewFixtureIdentity(fixture, previewCase) {
  if (!absolute(fixture) || !existsSync(fixture)) fail("fixture-invalid");
  assertPlainFixtureAncestors(fixture);
  const identityPath = path.join(fixture, "fixture-identity.json");
  let identity;
  try {
    identity = JSON.parse(readFileSync(identityPath, "utf8"));
  } catch {
    fail("identity-invalid");
  }
  if (
    !exactKeys(identity, ["schemaVersion", "case", "nonce", "files"]) ||
    identity.schemaVersion !== 1 ||
    identity.case !== caseName(previewCase) ||
    !SHA256.test(identity.nonce) ||
    !Array.isArray(identity.files) ||
    identity.files.length !== 3
  )
    fail("identity-invalid");
  const expected = [
    "av01-preview-case.json",
    "package.json",
    "preview-fixture.mjs",
  ];
  if (identity.files.map(({ name }) => name).join("\0") !== expected.join("\0"))
    fail("identity-invalid");
  for (const file of identity.files) {
    if (
      !exactKeys(file, ["name", "bytes", "sha256"]) ||
      !expected.includes(file.name) ||
      !Number.isSafeInteger(file.bytes) ||
      file.bytes < 1 ||
      file.bytes > MAX_FIXTURE_BYTES ||
      !SHA256.test(file.sha256) ||
      JSON.stringify(fileIdentity(path.join(fixture, file.name))) !==
        JSON.stringify(file)
    )
      fail("identity-invalid");
  }
  return Object.freeze(identity);
}

function parseArguments(argv) {
  if (!Array.isArray(argv) || argv.length !== 4) fail("arguments-invalid");
  const values = Object.create(null);
  for (let index = 0; index < argv.length; index += 2) {
    const key = argv[index]?.replace(/^--/u, "");
    if (!key || key in values) fail("arguments-invalid");
    values[key] = argv[index + 1];
  }
  if (!exactKeys(values, ["fixture", "preview-case"]))
    fail("arguments-invalid");
  return values;
}

if (
  process.argv[1] &&
  path.resolve(fileURLToPath(import.meta.url)) === path.resolve(process.argv[1])
) {
  try {
    const values = parseArguments(process.argv.slice(2));
    const identity = createAv01PreviewFixture({
      fixture: values.fixture,
      previewCase: values["preview-case"],
    });
    process.stdout.write(`${JSON.stringify(identity)}\n`);
  } catch (error) {
    console.error(String(error?.message ?? error).slice(0, 1024));
    process.exitCode = 1;
  }
}

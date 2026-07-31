import { readFile, readdir } from "node:fs/promises";
import path from "node:path";
import process from "node:process";

const root = process.cwd();
const failures = [];

async function readJson(relativePath) {
  return JSON.parse(await readFile(path.join(root, relativePath), "utf8"));
}

function fail(message) {
  failures.push(message);
}

async function filesBelow(relativeDirectory) {
  const directory = path.join(root, relativeDirectory);
  const entries = await readdir(directory, { withFileTypes: true });
  const files = [];

  for (const entry of entries) {
    const relativePath = path.join(relativeDirectory, entry.name);
    if (entry.isDirectory()) {
      files.push(...(await filesBelow(relativePath)));
    } else {
      files.push(relativePath);
    }
  }

  return files;
}

const tauriConfig = await readJson("src-tauri/tauri.conf.json");
const csp = tauriConfig.app?.security?.csp;

if (csp === null || csp === undefined) {
  fail("Tauri CSP must be enabled.");
}

if (typeof csp === "string" && csp.trim().length === 0) {
  fail("Tauri CSP must not be empty.");
}

const capabilities = await readJson("src-tauri/capabilities/main.json");
const allowedPermissions = new Set(["core:default"]);

for (const permission of capabilities.permissions ?? []) {
  const identifier =
    typeof permission === "string" ? permission : permission.identifier;
  if (!allowedPermissions.has(identifier)) {
    fail(`Unexpected Tauri permission: ${identifier}`);
  }
}

const packageJson = await readJson("package.json");
const allPackages = {
  ...(packageJson.dependencies ?? {}),
  ...(packageJson.devDependencies ?? {}),
};
const forbiddenPackages = [
  "electron",
  "monaco-editor",
  "node-pty",
  "simple-git",
  "isomorphic-git",
];

for (const dependency of forbiddenPackages) {
  if (dependency in allPackages) {
    fail(`Forbidden core dependency: ${dependency}`);
  }
}

const cargoToml = await readFile(
  path.join(root, "src-tauri/Cargo.toml"),
  "utf8",
);
const viteConfig = await readFile(path.join(root, "vite.config.ts"), "utf8");

if (/\bsourcemap\s*:\s*true\b/u.test(viteConfig)) {
  fail(
    "Production source maps must be explicitly opt-in, not unconditionally packaged.",
  );
}

if (/^\s*git2\s*=/m.test(cargoToml)) {
  fail("git2 is forbidden; Sage uses the installed Git CLI.");
}

if (
  !/agent-client-protocol\s*=\s*\{[^}]*version\s*=\s*"=1\./s.test(cargoToml)
) {
  fail("The production baseline must remain on an exact ACP v1 crate.");
}

if (
  !/agent-client-protocol\s*=\s*\{[^}]*optional\s*=\s*true/s.test(cargoToml)
) {
  fail("The ACP SDK must stay behind the isolated acp-runtime feature.");
}

const productionFiles = [
  ...(await filesBelow("src")),
  ...(await filesBelow("src-tauri/src")),
].filter(
  (file) =>
    /\.(?:ts|tsx|rs)$/.test(file) &&
    !/\.test\.tsx?$/u.test(file) &&
    !/(?:^|[\\/])tests\.rs$/u.test(file),
);

for (const file of productionFiles) {
  const source = await readFile(path.join(root, file), "utf8");
  // Rust commonly keeps unit tests in a cfg-gated child module. Those lines
  // are not linked into production and should not mask an oversized runtime
  // module; the runtime portion remains subject to the same 500-line budget.
  const productionSource = file.endsWith(".rs")
    ? source.split(/\r?\n#\[cfg\(test\)\]\r?\nmod tests \{/u, 1)[0]
    : source;
  const lineCount = productionSource.split(/\r?\n/).length;
  if (lineCount > 500) {
    fail(
      `${file} has ${lineCount} lines; split the module or document an exception.`,
    );
  }
}

if (failures.length > 0) {
  for (const failure of failures) {
    console.error(`policy: ${failure}`);
  }
  process.exitCode = 1;
} else {
  console.log(
    `policy: passed (${productionFiles.length} production modules checked)`,
  );
}

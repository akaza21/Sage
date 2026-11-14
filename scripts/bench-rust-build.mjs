// Times Sage's Rust edit loop, so every build-layout change can be compared
// with main on the same machine:
//
//   node scripts/bench-rust-build.mjs [--cold] [--edit <file>]...
//
// --cold runs `cargo clean` first and times the full test build; it refuses a
// target directory outside the checkout, which other lanes may share. Each --edit
// file (default: one git, one Codex binding, one image and one IPC module)
// gets a comment line appended, is rebuilt with `cargo test --no-run` and
// `cargo check --all-targets`, then is restored byte for byte.
import { spawnSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";
import { testFeatures } from "./cargo-features.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

const defaultEdits = [
  "crates/sage-git/src/parse.rs",
  "src-tauri/src/acp/bindings/codex/degrade.rs",
  "src-tauri/src/image_attachment.rs",
  "src-tauri/src/ipc_commands.rs",
];

const { values } = parseArgs({
  options: {
    cold: { type: "boolean", default: false },
    edit: { type: "string", multiple: true },
  },
});
const edits = values.edit ?? defaultEdits;
const features = ["--features", testFeatures().join(",")];
const testBuild = ["test", "--workspace", ...features, "--no-run"];
const checkBuild = ["check", "--workspace", ...features, "--all-targets"];

const results = [];

// Without a listener Node exits at once on Ctrl+C and skips the `finally`
// that restores an edited file. With one, the signal reaches cargo (same
// process group), the build fails, and the edit is undone before exiting.
let interrupted = false;
for (const signal of ["SIGINT", "SIGTERM"]) {
  process.on(signal, () => {
    interrupted = true;
  });
}

function cargo(label, args) {
  const started = process.hrtime.bigint();
  const run = spawnSync("cargo", args, {
    cwd: root,
    stdio: ["ignore", "ignore", "inherit"],
  });
  const seconds = Number(process.hrtime.bigint() - started) / 1e9;
  if (interrupted) throw new Error(`${label}: interrupted`);
  if (run.error) throw run.error;
  if (run.status !== 0) {
    throw new Error(`${label}: cargo ${args.join(" ")} exited ${run.status}`);
  }
  results.push({ label, seconds });
  console.error(`[bench] ${label}: ${seconds.toFixed(1)} s`);
}

function withEdit(file, build) {
  const absolute = path.resolve(root, file);
  const original = readFileSync(absolute);
  writeFileSync(
    absolute,
    Buffer.concat([original, Buffer.from(`// bench ${Date.now()}\n`)]),
  );
  try {
    build();
  } finally {
    writeFileSync(absolute, original);
  }
}

function targetDirectory() {
  const metadata = spawnSync(
    "cargo",
    ["metadata", "--format-version", "1", "--no-deps"],
    { cwd: root, encoding: "utf8", maxBuffer: 64 * 1024 * 1024 },
  );
  if (metadata.status !== 0) throw new Error("cargo metadata failed");
  return path.resolve(JSON.parse(metadata.stdout).target_directory);
}

if (values.cold) {
  const target = targetDirectory();
  const relative = path.relative(root, target);
  if (relative.startsWith("..") || path.isAbsolute(relative)) {
    throw new Error(`--cold would clean a shared target directory: ${target}`);
  }
  cargo("cargo clean", ["clean"]);
  cargo("cold test build", testBuild);
}
cargo("no-op test build", testBuild);
for (const file of edits) {
  withEdit(file, () => cargo(`edit ${file}: test build`, testBuild));
}
// Settle the check artifacts once, so each edit below times only itself.
cargo("check build", checkBuild);
for (const file of edits) {
  withEdit(file, () => cargo(`edit ${file}: check`, checkBuild));
}
// Rebuild against the restored sources, leaving the target dir current.
cargo("restored test build", testBuild);

console.log("| Step | Seconds |\n|---|---:|");
for (const { label, seconds } of results) {
  console.log(`| ${label} | ${seconds.toFixed(1)} |`);
}

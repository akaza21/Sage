import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";
import test from "node:test";

const workflowPath = path.join(process.cwd(), ".github/workflows/ci.yml");

function jobBlock(workflow, jobId) {
  const job = workflow.match(
    new RegExp(
      `^  ${jobId}:\\n([\\s\\S]*?)(?=^  [a-z][a-z0-9-]*:\\n|(?![\\s\\S]))`,
      "m",
    ),
  );
  assert.ok(job, `CI must define the ${jobId} job`);
  return job[0];
}

test("BETA-0001 macOS CI compiles and tests the named shared contracts", async () => {
  const workflow = await readFile(workflowPath, "utf8");
  const rust = jobBlock(workflow, "rust");
  const macosSharedContracts = jobBlock(workflow, "macos-shared-contracts");
  const mergeGate = jobBlock(workflow, "merge-gate");

  assert.match(
    rust,
    /matrix:[\s\S]*?os:[\s\S]*?-\s*windows-latest[\s\S]*?-\s*ubuntu-latest[\s\S]*?-\s*macos-latest/,
  );
  for (const command of [
    "cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check",
    "cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings",
    "cargo test --manifest-path src-tauri/Cargo.toml",
    "cargo check --manifest-path src-tauri/Cargo.toml --release --lib",
  ]) {
    assert.ok(
      rust.includes(command),
      `full Rust matrix must retain ${command}`,
    );
  }
  assert.match(macosSharedContracts, /runs-on:\s*macos-latest/);
  assert.match(
    macosSharedContracts,
    /name:\s*Compile shared contracts[\s\S]*?cargo check --manifest-path src-tauri\/Cargo\.toml --release --lib/,
  );
  assert.match(
    macosSharedContracts,
    /name:\s*Test workspace-notify shared contract[\s\S]*?cargo test --manifest-path src-tauri\/Cargo\.toml workspace::notify/,
  );
  assert.match(
    macosSharedContracts,
    /name:\s*Test editor-watch shared contract[\s\S]*?cargo test --manifest-path src-tauri\/Cargo\.toml editor_watch/,
  );
  assert.match(
    mergeGate,
    /needs:[\s\S]*?-\s*frontend[\s\S]*?-\s*rust[\s\S]*?-\s*macos-shared-contracts/,
  );
  assert.match(
    mergeGate,
    /MACOS_SHARED_CONTRACTS:\s*\$\{\{\s*needs\.macos-shared-contracts\.result\s*\}\}/,
  );
  assert.match(mergeGate, /test "\$MACOS_SHARED_CONTRACTS" = "success"/);
});

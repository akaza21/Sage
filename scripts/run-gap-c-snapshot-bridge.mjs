// A clean checkout can build and run the bounded component-to-SQLite bridge.
// Constrained runners may supply an already built lib-test executable relative
// to this checkout, avoiding a second Cargo invocation during browser tests.
import { spawn } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../", import.meta.url));
const prebuilt = process.env.SAGE_GAP_C_NATIVE_TEST_BINARY;
const relativeBinary = prebuilt
  ? path.relative(root, path.resolve(root, prebuilt))
  : "";
if (
  prebuilt &&
  (path.isAbsolute(prebuilt) ||
    path.isAbsolute(relativeBinary) ||
    relativeBinary.startsWith(".."))
)
  throw new Error("The prebuilt snapshot test must be checkout-relative");
const testArgs = [
  "component_snapshot_bridge_reads_historical_sqlite_bytes",
  "--test-threads=2",
  "--nocapture",
];
const child = spawn(
  prebuilt ? path.resolve(root, prebuilt) : "cargo",
  prebuilt
    ? testArgs
    : [
        "test",
        "--manifest-path",
        "Cargo.toml",
        "--lib",
        "--jobs",
        "2",
        testArgs[0],
        "--",
        ...testArgs.slice(1),
      ],
  { cwd: path.join(root, "src-tauri"), stdio: "inherit", windowsHide: true },
);
child.on("error", (error) => {
  console.error(error.message);
  process.exitCode = 1;
});
child.on("exit", (code) => {
  process.exitCode = code ?? 1;
});

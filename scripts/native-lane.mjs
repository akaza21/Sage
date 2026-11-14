// Runs the WebView2 native lane (src-tauri/tests/native_lane). It needs
// Windows with the WebView2 runtime; anywhere else it fails rather than
// passing with zero tests.
import { spawnSync } from "node:child_process";

if (process.platform !== "win32") {
  console.error(
    `[native-lane] RESULT: FAIL (the native lane needs Windows and WebView2; this is ${process.platform})`,
  );
  process.exit(1);
}

const result = spawnSync(
  "cargo",
  ["test", "-p", "sage", "--features", "native-lane", "--test", "native_lane"],
  { stdio: "inherit", shell: false },
);
if (result.error) {
  console.error(
    `[native-lane] RESULT: FAIL (cargo did not start: ${result.error.message})`,
  );
  process.exit(1);
}
process.exit(result.status ?? 1);

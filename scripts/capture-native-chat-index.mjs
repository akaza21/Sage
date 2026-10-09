import { spawn } from "node:child_process";
import { runTool } from "./capture-native-chat-process.mjs";

export function trackedIndexParser() {
  let pending = Buffer.alloc(0),
    count = 0;
  return {
    feed(chunk) {
      const bytes = Buffer.from(chunk);
      let start = 0;
      while (start < bytes.length) {
        const end = bytes.indexOf(0, start);
        const piece = bytes.subarray(start, end === -1 ? bytes.length : end);
        if (pending.length + piece.length > 4096)
          throw new Error("source-index-bound");
        pending = Buffer.concat([pending, piece]);
        if (end === -1) break;
        if (++count > 200000) throw new Error("source-index-bound");
        const tag = String.fromCharCode(pending[0]);
        if (pending.length < 3 || pending[1] !== 32 || !/^[A-Za-z]$/u.test(tag))
          throw new Error("source-index-record");
        if (tag === "S" || /^[a-z]$/u.test(tag))
          throw new Error("source-index-flags");
        pending = Buffer.alloc(0);
        start = end + 1;
      }
    },
    finish() {
      if (pending.length !== 0) throw new Error("source-index-record");
    },
  };
}
export function rejectIndexFlags(listing) {
  const parser = trackedIndexParser();
  parser.feed(listing);
  parser.finish();
}
export function readTrackedIndex(executable, args, launch = spawn) {
  return new Promise((resolve, reject) => {
    const child = launch(executable, args, {
      windowsHide: true,
      stdio: ["ignore", "pipe", "pipe"],
    });
    const parser = trackedIndexParser();
    let done = false,
      stderrBytes = 0;
    const finish = (error) => {
      if (done) return;
      done = true;
      clearTimeout(timer);
      if (error) {
        child.kill();
        reject(error);
      } else resolve("");
    };
    const timer = setTimeout(
      () => finish(new Error("source-index-timeout")),
      10000,
    );
    child.on("error", () => finish(new Error("source-index-tool")));
    child.stdout.on("data", (chunk) => {
      if (done) return;
      try {
        parser.feed(chunk);
      } catch (error) {
        finish(error);
      }
    });
    child.stderr.on("data", (chunk) => {
      if (done) return;
      stderrBytes += chunk.length;
      if (stderrBytes > 65536) finish(new Error("source-index-bound"));
    });
    child.on("close", (code) => {
      if (done) return;
      if (code !== 0) return finish(new Error("source-index-tool"));
      try {
        parser.finish();
        finish();
      } catch (error) {
        finish(error);
      }
    });
  });
}
export function sourceTool(executable, args) {
  return args[2] === "ls-files"
    ? readTrackedIndex(executable, args)
    : runTool(executable, args);
}

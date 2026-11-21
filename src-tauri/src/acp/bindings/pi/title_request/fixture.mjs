import { createInterface } from "node:readline";
import { spawn } from "node:child_process";
import { writeFileSync, appendFileSync } from "node:fs";
import { join } from "node:path";

const scenario = process.env.TITLE_FIXTURE_SCENARIO ?? process.argv[2];
const args = process.argv.slice(process.env.TITLE_FIXTURE_SCENARIO ? 2 : 3);
const required = [
  "--no-session",
  "--no-tools",
  "--no-extensions",
  "--no-mcp",
  "--no-skills",
  "--no-prompt-templates",
  "--no-context-files",
  "--no-themes",
];
if (
  scenario !== "main" &&
  (required.some((flag) => !args.includes(flag)) || args.includes("--session"))
)
  process.exit(9);
if (
  scenario !== "main" &&
  (args[args.indexOf("--provider") + 1] !== "fixture" ||
    args[args.indexOf("--model") + 1] !== "same-model")
)
  process.exit(10);
const input = createInterface({ input: process.stdin });
input.on("line", (line) => {
  const command = JSON.parse(line);
  if (["set_auto_retry", "set_auto_compaction"].includes(command.type)) {
    writeFileSync(
      join(process.env.TITLE_FIXTURE_PROFILE, "settings.json"),
      JSON.stringify(command),
    );
  }
  if (command.type !== "prompt") return;
  const sessionIndex = args.indexOf("--session");
  if (sessionIndex >= 0) appendFileSync(args[sessionIndex + 1], line + "\n");
  if (Buffer.byteLength(command.message) > 4096) process.exit(8);
  if (process.env.TITLE_FIXTURE_DIR) {
    appendFileSync(
      join(process.env.TITLE_FIXTURE_DIR, "isolated-input.jsonl"),
      line + "\n",
    );
  }
  if (scenario === "hang") {
    const child = spawn(
      process.execPath,
      ["-e", "setInterval(() => {}, 1000)"],
      { stdio: "ignore" },
    );
    writeFileSync(
      join(process.env.TITLE_FIXTURE_DIR, "child.pid"),
      String(child.pid),
    );
    return;
  }
  if (scenario === "malformed") {
    console.log("not json");
    return;
  }
  if (scenario === "nonzero") {
    process.exitCode = 7;
    process.stdin.destroy();
  }
  if (scenario === "stream-oversize") {
    console.log(
      JSON.stringify({
        type: "message_update",
        assistantMessageEvent: {
          type: "text_delta",
          delta: "x".repeat(1025),
        },
      }),
    );
  }
  console.log(
    JSON.stringify({
      type: "message_end",
      message: {
        role: "assistant",
        stopReason: "stop",
        content: [
          {
            type: "text",
            text:
              scenario === "oversize"
                ? "x".repeat(1025)
                : "Check Sage chat titles",
          },
        ],
      },
    }),
  );
  console.log(JSON.stringify({ type: "agent_settled" }));
});
input.on("close", () => {
  if (scenario === "eof-error") process.exitCode = 1;
});

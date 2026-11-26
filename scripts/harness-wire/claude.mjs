// Claude Code over stream-json, launched with Sage's SDK flags
// (src-tauri/src/acp/bindings/claude/sdk/admission.rs SDK_ARGS). Control
// request ids follow the bridge's own `sage-N` numbering.
import { randomUUID } from "node:crypto";
import { PROMPTS } from "./prompts.mjs";
import { Wire } from "./wire.mjs";

export const SDK_ARGS = [
  "--output-format",
  "stream-json",
  "--input-format",
  "stream-json",
  "--verbose",
  "--include-partial-messages",
  "--permission-prompt-tool",
  "stdio",
  "--permission-mode",
  "default",
  "--setting-sources",
  "user,project,local",
];

export const SCENARIOS = [
  "commands",
  "shell",
  "edit",
  "image",
  "subagent",
  "plan-approve",
  "plan-decline",
  "auth-fail",
];

export function launch({ cwd, env, model }) {
  const sessionId = randomUUID();
  const args = [...SDK_ARGS, "--session-id", sessionId];
  if (model) args.push("--model", model);
  return { command: "claude", args, cwd, env };
}

// Every scenario runs on a config home seeded with the sign-in alone, so no
// plugin, skill, agent, setting or connector of the capturing account is
// recorded; claude.ai connectors are switched off for the same reason.
export const CONFIG = { env: "CLAUDE_CONFIG_DIR", from: [".claude"], files: [".credentials.json"] };

export async function run(scenario, { cwd, env, emptyHome, configHome, image, model }) {
  const scenarioEnv = { ...env, CLAUDE_CONFIG_DIR: configHome, ENABLE_CLAUDEAI_MCP_SERVERS: "false" };
  if (scenario === "auth-fail") {
    scenarioEnv.CLAUDE_CONFIG_DIR = emptyHome;
    for (const key of ["ANTHROPIC_API_KEY", "CLAUDE_CODE_OAUTH_TOKEN", "ANTHROPIC_AUTH_TOKEN"]) {
      delete scenarioEnv[key];
    }
  }
  const spec = launch({ cwd, env: scenarioEnv, model });
  const wire = new Wire(spec);
  let next = 0;
  const control = (request, step) => {
    next += 1;
    const id = `sage-${next}`;
    wire.send({ type: "control_request", request_id: id, request }, step);
    return id;
  };
  const answered = (id) => (frame) =>
    frame.type === "control_response" && frame.response?.request_id === id;
  const decisions = [];
  const serve = (frame) => {
    if (frame.type !== "control_request") return;
    const request = frame.request ?? {};
    let response;
    if (request.subtype === "can_use_tool") {
      const deny =
        (scenario === "plan-decline" && request.tool_name === "ExitPlanMode") ||
        request.tool_name === "AskUserQuestion";
      response = deny
        ? { behavior: "deny", message: "The user declined." }
        : { behavior: "allow", updatedInput: request.input ?? {} };
      decisions.push(deny ? "deny" : "allow");
      wire.send(
        { type: "control_response", response: { subtype: "success", request_id: frame.request_id, response } },
        { command: "permission", decision: deny ? "deny" : "allow" },
      );
      return;
    }
    wire.send({
      type: "control_response",
      response: { subtype: "error", request_id: frame.request_id, error: "unsupported" },
    });
  };
  const result = (frame) => frame.type === "result";
  const prompt = async (text, { images = [], step = {} } = {}) => {
    const content = [{ type: "text", text }];
    for (const data of images) {
      content.push({ type: "image", source: { type: "base64", media_type: "image/png", data } });
    }
    wire.send(
      {
        type: "user",
        session_id: spec.args[spec.args.indexOf("--session-id") + 1],
        message: { role: "user", content },
        parent_tool_use_id: null,
      },
      { command: "prompt", text, images: images.length, ...step },
    );
    return wire.until(result, { serve });
  };
  let outcome = "completed";
  try {
    const init = control(
      { subtype: "initialize", perTaskStopAffordance: true, forwardSubagentText: true },
      { command: "initialize" },
    );
    await wire.until(answered(init), { serve, timeoutMs: 120000 });
    const settings = control({ subtype: "get_settings" }, { command: "session" });
    await wire.until(answered(settings), { serve, timeoutMs: 60000 });
    switch (scenario) {
      case "commands":
        break;
      case "image":
        await prompt(PROMPTS.image, { images: [image] });
        break;
      case "plan-approve":
      case "plan-decline": {
        const mode = control({ subtype: "set_permission_mode", mode: "plan" }, { command: "mode", mode: "plan" });
        await wire.until(answered(mode), { serve });
        await prompt(PROMPTS.plan, { step: { mode: "plan" } });
        break;
      }
      case "auth-fail":
        await prompt(PROMPTS.hello);
        break;
      default:
        await prompt(PROMPTS[scenario]);
    }
  } catch (error) {
    outcome = `error: ${error.message}`;
  }
  await wire.close(scenario === "shell" || scenario === "subagent" ? 8000 : 1500);
  return { wire, outcome, decisions, args: spec.args.filter((arg) => !/^[0-9a-f-]{36}$/.test(arg)) };
}

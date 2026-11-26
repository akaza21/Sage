// Codex app-server JSON-RPC, with the initialize and turn policy Sage sends
// (src-tauri/src/acp/bindings/codex/request.rs and turn_policy.rs). Sage's
// Codex binding has no plan access mode, so plan scenarios are a declared gap.
import fs from "node:fs";
import { PROMPTS } from "./prompts.mjs";
import { Wire } from "./wire.mjs";

export const SCENARIOS = ["commands", "shell", "edit", "image", "subagent", "auth-fail"];
export const GAPS = {
  "plan-approve": "Sage's Codex binding admits no plan access mode",
  "plan-decline": "Sage's Codex binding admits no plan access mode",
};

const ASK = { approval: "on-request", sandbox: "readOnly" };
// Sage names itself with its crate version, which Codex echoes in userAgent.
const SAGE_VERSION = /^version = "([^"]+)"/m.exec(fs.readFileSync(new URL("../../src-tauri/Cargo.toml", import.meta.url), "utf8"))?.[1] ?? "0.0.0";

export function launch({ cwd, env }) {
  return { command: "codex", args: ["app-server"], cwd, env, shell: process.platform === "win32" };
}

export async function run(scenario, { cwd, env, emptyHome, image }) {
  const scenarioEnv = { ...env };
  if (scenario === "auth-fail") {
    scenarioEnv.CODEX_HOME = emptyHome;
    delete scenarioEnv.OPENAI_API_KEY;
    delete scenarioEnv.CODEX_API_KEY;
  }
  const spec = launch({ cwd, env: scenarioEnv });
  const wire = new Wire(spec);
  let next = 0;
  const request = (method, params, step) => {
    next += 1;
    wire.send({ id: next, method, params }, step);
    return next;
  };
  const answered = (id) => (frame) => frame.id === id && !frame.method;
  const decisions = [];
  const serve = (frame) => {
    if (frame.id === undefined || !frame.method) return;
    if (/requestApproval$/.test(frame.method)) {
      decisions.push("accept");
      wire.send({ id: frame.id, result: { decision: "accept" } }, { command: "permission", decision: "allow" });
      return;
    }
    wire.send({ id: frame.id, error: { code: -32601, message: "unsupported" } });
  };
  let thread = null;
  const turn = async (text, input) => {
    const id = request(
      "turn/start",
      {
        threadId: thread,
        clientUserMessageId: `client-${next + 1}`,
        input,
        approvalPolicy: ASK.approval,
        sandboxPolicy: { type: ASK.sandbox, networkAccess: false },
      },
      { command: "prompt", text, images: input.length - 1 },
    );
    const reply = await wire.until(answered(id), { serve });
    if (reply.error) return reply;
    return wire.until((frame) => frame.method === "turn/completed", { serve });
  };
  let outcome = "completed";
  try {
    const init = request(
      "initialize",
      {
        clientInfo: { name: "sage", title: "Sage", version: SAGE_VERSION },
        capabilities: {
          experimentalApi: true,
          optOutNotificationMethods: ["turn/diff/updated", "item/fileChange/outputDelta"],
        },
      },
      { command: "initialize" },
    );
    await wire.until(answered(init), { serve, timeoutMs: 120000 });
    wire.send({ method: "initialized" });
    const start = request(
      "thread/start",
      { model: null, cwd, approvalPolicy: "never", sandbox: "read-only", ephemeral: false },
      { command: "session" },
    );
    const started = await wire.until(answered(start), { serve, timeoutMs: 120000 });
    thread = started.result?.thread?.id ?? null;
    if (!thread) throw new Error("thread/start returned no thread");
    const text = (value) => [{ type: "text", text: value, text_elements: [] }];
    switch (scenario) {
      case "commands": {
        const skills = request("skills/list", { cwds: [cwd] });
        await wire.until(answered(skills), { serve });
        const models = request("model/list", {});
        await wire.until(answered(models), { serve });
        break;
      }
      case "image":
        await turn(PROMPTS.image, [
          ...text(PROMPTS.image),
          { type: "image", url: `data:image/png;base64,${image}` },
        ]);
        break;
      case "auth-fail":
        await turn(PROMPTS.hello, text(PROMPTS.hello));
        break;
      default:
        await turn(PROMPTS[scenario], text(PROMPTS[scenario]));
    }
  } catch (error) {
    outcome = `error: ${error.message}`;
  }
  await wire.close(scenario === "shell" ? 6000 : 1500);
  return { wire, outcome, decisions, args: spec.args };
}

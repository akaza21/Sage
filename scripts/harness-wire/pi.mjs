// Pi over `--mode rpc` (src-tauri/src/acp/bindings/pi/manifest.rs
// PI_RPC_ARGS), with Pi's own built-in tools. Sage's native Pi lane swaps
// those for its bridge extension, so turns here record Pi's event protocol
// rather than the bridge. Ids follow the translator's `pi-<epoch>-<n>`.
import { PROMPTS } from "./prompts.mjs";
import { Wire } from "./wire.mjs";

export const SCENARIOS = ["commands", "shell", "edit", "image", "auth-fail"];
export const GAPS = {
  subagent: "Pi has no built-in subagent tool",
  "plan-approve": "Pi has no plan mode",
  "plan-decline": "Pi has no plan mode",
};

export function launch({ cwd, env }) {
  return { command: "pi", args: ["--mode", "rpc"], cwd, env, shell: process.platform === "win32" };
}

export async function run(scenario, { cwd, env, emptyHome, image }) {
  const scenarioEnv = { ...env };
  if (scenario === "auth-fail") {
    scenarioEnv.PI_CODING_AGENT_DIR = emptyHome;
    for (const key of ["ANTHROPIC_API_KEY", "OPENAI_API_KEY", "GEMINI_API_KEY", "OPENROUTER_API_KEY"]) {
      delete scenarioEnv[key];
    }
  }
  const spec = launch({ cwd, env: scenarioEnv });
  const wire = new Wire(spec);
  let next = 0;
  const command = (type, fields = {}, step) => {
    next += 1;
    const id = `pi-1-${next}`;
    wire.send({ ...fields, type, id }, step);
    return id;
  };
  const answered = (id) => (frame) => frame.type === "response" && frame.id === id;
  const serve = (frame) => {
    if (frame.type !== "extension_ui_request" || !frame.id) return;
    // Dialogs need an answer; fire-and-forget notices do not.
    if (["select", "confirm", "input", "editor"].includes(frame.method)) {
      wire.send({ type: "extension_ui_response", id: frame.id, cancelled: true });
    }
  };
  const prompt = async (text, images = []) => {
    const id = command(
      "prompt",
      { message: text, ...(images.length ? { images } : {}) },
      { command: "prompt", text, images: images.length },
    );
    const reply = await wire.until(answered(id), { serve });
    if (reply.success === false) return reply;
    return wire.until((frame) => frame.type === "agent_end", { serve });
  };
  let outcome = "completed";
  try {
    const state = command("get_state", {}, { command: "initialize" });
    const models = command("get_available_models");
    await wire.until(answered(state), { serve, timeoutMs: 120000 });
    await wire.until(answered(models), { serve, timeoutMs: 60000 });
    const session = command("get_state", {}, { command: "session" });
    await wire.until(answered(session), { serve });
    const commands = command("get_commands");
    await wire.until(answered(commands), { serve });
    switch (scenario) {
      case "commands":
        break;
      case "image":
        await prompt(PROMPTS.image, [{ type: "image", data: image, mimeType: "image/png" }]);
        break;
      case "auth-fail":
        await prompt(PROMPTS.hello);
        break;
      default:
        await prompt(PROMPTS[scenario]);
    }
  } catch (error) {
    outcome = `error: ${error.message}`;
  }
  await wire.close(scenario === "shell" ? 6000 : 2500);
  return { wire, outcome, decisions: [], args: spec.args };
}

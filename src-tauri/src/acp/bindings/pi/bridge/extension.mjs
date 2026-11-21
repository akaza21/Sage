// Sage Pi bridge v1; admitted only with Pi 1.5.0 and exact installed bytes.
// Sage writes approved files; reads use Pi's own read-only implementation.
// Pi's own bash runs a command only after Sage records the user's approval.
import {
  createBashToolDefinition,
  createReadToolDefinition,
} from "@earendil-works/pi-coding-agent";
import { fileURLToPath } from "node:url";
const MAX_BYTES = 262144;
// Every answer other than "approved" or "written" is Sage's reason, shown to
// the model and the user as the tool's error.
const refusal = (answer) =>
  new Error(
    typeof answer === "string" && answer
      ? answer.slice(0, 64)
      : "Sage did not answer the approval request",
  );
const fields = (names) =>
  Object.fromEntries(names.map((name) => [name, { type: "string" }]));
// Pi passes this same graph to later hooks and then execute(). Freeze before
// asking Sage, including nested objects: approval must describe execution.
function freezeInput(value) {
  if (value === null || typeof value !== "object") return;
  Object.freeze(value);
  for (const child of Object.values(value)) freezeInput(child);
}
// Hold only the approval transaction, never the tool execution. At most eight
// callers wait; cancellation removes their entries and listeners immediately.
function approvalQueue() {
  let active = false;
  const waiting = [];
  return async (signal, operation) => {
    if (signal?.aborted) throw new Error("Cancelled");
    await new Promise((resolve, reject) => {
      const start = () => {
        signal?.removeEventListener("abort", cancel);
        resolve();
      };
      const cancel = () => {
        const index = waiting.indexOf(start);
        if (index >= 0) waiting.splice(index, 1);
        reject(new Error("Cancelled"));
      };
      if (!active) {
        active = true;
        start();
      } else if (waiting.length >= 8) {
        reject(new Error("Sage approval queue is full (8 waiting)"));
      } else {
        waiting.push(start);
        signal?.addEventListener("abort", cancel, { once: true });
      }
    });
    try {
      if (signal?.aborted) throw new Error("Cancelled");
      return await operation();
    } finally {
      const next = waiting.shift();
      if (next) next();
      else active = false;
    }
  };
}
// Resolves once Sage records the user's approval of this tool call; throws
// Sage's reason otherwise.
async function approve(toolCallId, tool, args, signal, ctx, metadata = {}) {
  if (signal?.aborted) throw new Error("Cancelled");
  const request = JSON.stringify({
    version: 1,
    toolCallId,
    tool,
    args,
    ...metadata,
  });
  if (Buffer.byteLength(request) > MAX_BYTES)
    throw new Error("Sage operation exceeds the byte limit");
  freezeInput(args);
  let approved = await ctx.ui.input("sage.pi.prepare.v1", request, {
    signal,
    timeout: 60000,
  });
  // Pi expires or aborts a dialog without telling Sage, so Sage settles
  // it: an undecided request is withdrawn, a recorded approval stands.
  if (approved === undefined)
    approved = await ctx.ui.input("sage.pi.withdraw.v1", toolCallId, {
      timeout: 10000,
    });
  if (signal?.aborted) throw new Error("Cancelled");
  if (approved !== "approved") throw refusal(approved);
}
export default function sageBridge(pi) {
  const queued = approvalQueue();
  const source = fileURLToPath(import.meta.url);
  pi.on("tool_call", async (event, ctx) => {
    const tool = pi.getAllTools().find((tool) => tool.name === event.toolName);
    if (
      (tool?.sourceInfo?.path === source &&
        ["read", "write", "edit", "bash"].includes(event.toolName)) ||
      tool?.sourceInfo?.path === "builtin:read"
    )
      return;
    try {
      await queued(ctx.signal, () =>
        approve(
          event.toolCallId,
          event.toolName,
          event.input,
          ctx.signal,
          ctx,
          {
            kind: "extension",
            label: tool?.label,
          },
        ),
      );
    } catch (error) {
      return { block: true, reason: refusal(error.message).message };
    }
  });
  // getAllTools is bound only after discovery. Register then, so a discovered
  // owner keeps its name and is gated above rather than colliding at startup.
  pi.on("session_start", () => {
    const managed = [];
    const register = (definition) => {
      const owner = pi
        .getAllTools()
        .find((tool) => tool.name === definition.name);
      if (
        owner &&
        owner.sourceInfo?.path !== source &&
        owner.sourceInfo?.path !== `builtin:${definition.name}`
      )
        return;
      pi.registerTool(definition);
      managed.push(definition.name);
    };
    register(createReadToolDefinition(process.cwd()));
    for (const [name, required] of [
      ["write", ["path", "content"]],
      ["edit", ["path", "oldText", "newText"]],
    ]) {
      register({
        name,
        label: name,
        description:
          "Sage-managed operation, subject to workspace policy and approval.",
        parameters: {
          type: "object",
          properties: fields(required),
          required,
          additionalProperties: false,
        },
        async execute(toolCallId, args, signal, _update, ctx) {
          await queued(signal, async () => {
            await approve(toolCallId, name, args, signal, ctx);
            const result = await ctx.ui.input("sage.pi.commit.v1", toolCallId, {
              signal,
              timeout: 60000,
            });
            if (result !== "written") throw refusal(result);
          });
          return {
            content: [
              { type: "text", text: "Workspace file updated by Sage." },
            ],
            details: {},
          };
        },
      });
    }
    // Pi's bash, except that each command first needs approval. Pi's shell
    // path and command prefix settings are not applied, so the command that
    // runs is exactly the one the user approved.
    const shell = createBashToolDefinition(process.cwd());
    register({
      ...shell,
      async execute(toolCallId, args, signal, update, ctx) {
        await queued(signal, () =>
          approve(toolCallId, "bash", args, signal, ctx),
        );
        return shell.execute(toolCallId, args, signal, update, ctx);
      },
    });
    // Pi retains inactive built-in registry entries. Replacing one at runtime
    // does not activate it, so include our (at most four) tools explicitly.
    pi.setActiveTools([...new Set([...pi.getActiveTools(), ...managed])]);
  });
}

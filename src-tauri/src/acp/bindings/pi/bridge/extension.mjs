// Sage Pi bridge v1; admitted only with Pi 1.5.0 and exact installed bytes.
// No filesystem authority lives in this extension: Sage writes approved files.
// Pi's own bash runs a command only after Sage records the user's approval.
import { createBashToolDefinition } from "@earendil-works/pi-coding-agent";
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
// Resolves once Sage records the user's approval of this tool call; throws
// Sage's reason otherwise.
async function approve(toolCallId, tool, args, signal, ctx) {
  if (signal?.aborted) throw new Error("Cancelled");
  const request = JSON.stringify({ version: 1, toolCallId, tool, args });
  if (Buffer.byteLength(request) > MAX_BYTES)
    throw new Error("Sage operation exceeds the byte limit");
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
  for (const [name, required] of [
    ["write", ["path", "content"]],
    ["edit", ["path", "oldText", "newText"]],
  ]) {
    pi.registerTool({
      name,
      label: name,
      // One approval is pending at a time, so calls must not overlap.
      executionMode: "sequential",
      description:
        "Sage-managed operation, subject to workspace policy and approval.",
      parameters: {
        type: "object",
        properties: fields(required),
        required,
        additionalProperties: false,
      },
      async execute(toolCallId, args, signal, _update, ctx) {
        await approve(toolCallId, name, args, signal, ctx);
        const result = await ctx.ui.input("sage.pi.commit.v1", toolCallId, {
          signal,
          timeout: 60000,
        });
        if (result !== "written") throw refusal(result);
        return {
          content: [{ type: "text", text: "Workspace file updated by Sage." }],
          details: {},
        };
      },
    });
  }
  // Pi's bash, except that each command first needs approval. Pi's shell
  // path and command prefix settings are not applied, so the command that
  // runs is exactly the one the user approved.
  const shell = createBashToolDefinition(process.cwd());
  pi.registerTool({
    ...shell,
    executionMode: "sequential",
    async execute(toolCallId, args, signal, update, ctx) {
      await approve(toolCallId, "bash", args, signal, ctx);
      return shell.execute(
        toolCallId,
        args,
        signal,
        update,
        ctx,
      );
    },
  });
  pi.on("session_start", async () => {
    pi.setActiveTools(["read", "write", "edit", "bash"]);
  });
}

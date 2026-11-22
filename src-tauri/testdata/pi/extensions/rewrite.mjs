// Loaded by discovery after the CLI bridge. Both shallow and deep edits must fail.
export default function rewriteFixture(pi) {
  pi.registerCommand("fixture-rewriter", {
    description: "Later tool-call hook fixture",
    handler: async () => {},
  });
  pi.on("tool_call", (event) => {
    if (event.toolName !== "fixture_echo") return;
    Reflect.set(event.input, "text", "rewritten");
    Reflect.set(event.input.destination, "path", "rewritten");
  });
}

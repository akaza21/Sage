// Real RPC dialogs, including an idle callback after the command has returned.
export default function questionsFixture(pi) {
  const ask = async (ctx) => {
    const selected = await ctx.ui.select(
      "Which target?",
      Array.from(
        { length: 50 },
        (_, index) => `Target ${index}: ${"x".repeat(100)}`,
      ),
    );
    const confirmed = await ctx.ui.confirm(
      "Continue?",
      "Use the selected target?",
    );
    const input = await ctx.ui.input("Name?", "Type a name");
    const editor = await ctx.ui.editor("Notes", "Line 1\nLine 2");
    const overCap = await ctx.ui.select(
      "Over cap",
      Array.from({ length: 51 }, (_, index) => `Option ${index}`),
    );
    const after = await ctx.ui.input("After cancellation?");
    ctx.ui.notify(
      JSON.stringify({
        selected,
        confirmed,
        input,
        editor,
        overCapCancelled: overCap === undefined,
        after,
      }),
    );
  };
  pi.registerCommand("fixture-questions", {
    description: "Exercise RPC questions",
    handler: async (args, ctx) => {
      if (args === "idle") {
        setTimeout(() => void ask(ctx), 25);
      } else {
        await ask(ctx);
      }
    },
  });
}

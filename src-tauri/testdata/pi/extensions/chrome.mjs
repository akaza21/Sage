// Real fire-and-forget extension UI past Sage's caps, then cleared.
export default function chromeFixture(pi) {
  pi.registerCommand("fixture-chrome", {
    description: "Exercise RPC status, widget, editor text and title",
    handler: async (args, ctx) => {
      const statuses = Array.from({ length: 9 }, (_, index) => `s${index}`);
      const widgets = Array.from({ length: 4 }, (_, index) => `w${index}`);
      if (args === "caps") {
        for (const key of statuses) ctx.ui.setStatus(key, `Status ${key}`);
        ctx.ui.setStatus("s0", "y".repeat(300));
        ctx.ui.setWidget(
          "w0",
          Array.from({ length: 12 }, (_, index) =>
            index === 0 ? "z".repeat(300) : `Line ${index}`,
          ),
          { placement: "belowEditor" },
        );
        for (const key of widgets.slice(1)) ctx.ui.setWidget(key, [key]);
        ctx.ui.setWidget("not-text", [42]);
        ctx.ui.setEditorText("x".repeat(17 * 1024));
      } else {
        for (const key of [...statuses, "rpc-demo"])
          ctx.ui.setStatus(key, undefined);
        for (const key of [...widgets, "rpc-demo"])
          ctx.ui.setWidget(key, undefined);
        ctx.ui.setEditorText("");
        ctx.ui.setTitle("");
      }
    },
  });
}

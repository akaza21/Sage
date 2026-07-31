import { FileCode2, TerminalSquare } from "lucide-react";

import type { WorkbenchTab } from "../domain/workbench";

export function PlaceholderSurface({ tab }: { tab: WorkbenchTab }) {
  const terminal = tab.kind === "terminal";
  const Icon = terminal ? TerminalSquare : FileCode2;

  return (
    <section className="placeholder-surface">
      <Icon size={32} strokeWidth={1.4} aria-hidden="true" />
      <h1>{tab.title}</h1>
      <p>
        {terminal
          ? "The Rust PTY supervisor will mount this terminal only while its tab is active."
          : "CodeMirror will mount source files here without creating an automatic split."}
      </p>
    </section>
  );
}

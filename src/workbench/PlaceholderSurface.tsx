import { FileCode2 } from "lucide-react";

import type { WorkbenchTab } from "../domain/workbench";

export function PlaceholderSurface({ tab }: { tab: WorkbenchTab }) {
  return (
    <section className="placeholder-surface">
      <FileCode2 size={32} strokeWidth={1.4} aria-hidden="true" />
      <h1>{tab.title}</h1>
      <p>
        This legacy tab type is unavailable in this beta. Close it and use the
        current workbench controls.
      </p>
    </section>
  );
}

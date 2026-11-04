import { useWorkbenchStore } from "../../app/store";
import { resourceKey } from "../../domain/resourceIdentity";
import { outlineSummaryTitle } from "./outlineCard";
import { OutlineTree } from "./OutlineTree";
import { useSyntaxOutline, type ActiveOutline } from "./useSyntaxOutline";

export function OutlineSurface() {
  const projectId = useWorkbenchStore((state) => state.project?.id ?? null);
  const transitionEpoch = useWorkbenchStore(
    (state) => state.nativeTransitionEpoch,
  );
  const active = useSyntaxOutline();
  const outline = active?.status === "ready" ? active.outline : null;
  const symbols = outline?.symbols ?? [];
  const resource = active?.resource;

  return (
    <section className="outline-surface" aria-label="Outline">
      <header className="outline-surface__header">
        <h1>Outline</h1>
        <span>{outlineSummaryTitle(symbols.length, outline?.truncated)}</span>
      </header>
      {resource && symbols.length ? (
        <OutlineTree
          key={resourceKey(resource)}
          symbols={symbols}
          onOpen={(symbol) => {
            if (!projectId) return;
            void useWorkbenchStore.getState().openFileLocation({
              projectId,
              transitionEpoch,
              resource,
              lineNumber: symbol.line,
              columnNumber: symbol.column,
              selection: { kind: "caret" },
            });
          }}
          location={resource.relativePath}
        />
      ) : (
        <p className="outline-surface__empty" role="status">
          {outlineEmptyMessage(active?.status)}
        </p>
      )}
    </section>
  );
}

function outlineEmptyMessage(status: ActiveOutline["status"] | undefined) {
  switch (status) {
    case "reading":
      return "Reading symbols…";
    case "unsupported":
      return "No Outline for this File type.";
    case "too-large":
      return "This File is too large for an Outline.";
    case "ready":
      return "No symbols for the active File.";
    case undefined:
      return "No File is open.";
  }
}

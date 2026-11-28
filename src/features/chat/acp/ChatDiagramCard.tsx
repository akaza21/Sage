import { ChevronRight, Workflow } from "../../../ui/leanIcons";
import { openAgentPaneView } from "../../../workbench/agentPaneViews";

const KIND: Record<string, string> = {
  graph: "Flowchart",
  flowchart: "Flowchart",
  sequencediagram: "Sequence diagram",
  classdiagram: "Class diagram",
  statediagram: "State diagram",
  "statediagram-v2": "State diagram",
};

export function diagramTitle(source: string) {
  const head = /^\s*(\S+)/u.exec(source)?.[1]?.toLowerCase() ?? "";
  return KIND[head] ?? "Diagram";
}

function sourceKey(source: string) {
  let hash = 5381;
  for (let index = 0; index < source.length; index += 1)
    hash = (hash * 33) ^ source.charCodeAt(index);
  return `${(hash >>> 0).toString(36)}:${source.length}`;
}

/** A diagram in a message is a compact card; it opens in the right pane. */
export function ChatDiagramCard({
  source,
  chatId,
}: {
  source: string;
  chatId: string;
}) {
  const title = diagramTitle(source);
  return (
    <button
      className="acp-diagram-card"
      type="button"
      aria-label={`Open ${title} in the side pane`}
      onClick={() =>
        openAgentPaneView(
          { chatId, kind: "diagram", title, source },
          sourceKey(source),
        )
      }
    >
      <Workflow size={16} aria-hidden="true" />
      <span>{title}</span>
      <ChevronRight size={14} aria-hidden="true" />
    </button>
  );
}

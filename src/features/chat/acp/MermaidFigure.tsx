import { parseMermaidSource } from "./mermaidDiagram";
import type { MermaidDiagram, MermaidSource } from "./mermaidDiagram";
import { StateSvg } from "./mermaidStateFigure";

export function MermaidFigure({ source }: { source: string }) {
  const diagram = parseMermaidSource(source);
  return (
    <figure className="acp-mermaid">
      {diagram.ok && diagram.kind === "flowchart" ? (
        <MermaidSvg diagram={diagram} />
      ) : diagram.ok && diagram.kind === "sequence" ? (
        <SequenceSvg diagram={diagram} />
      ) : diagram.ok && diagram.kind === "class" ? (
        <ClassSvg diagram={diagram} />
      ) : diagram.ok && diagram.kind === "state" ? (
        <StateSvg diagram={diagram} />
      ) : (
        <p className="acp-mermaid__fallback">
          Diagram source is kept for copy; this mermaid subset could not be
          drawn.
        </p>
      )}
      <button
        type="button"
        className="acp-mermaid__copy"
        onClick={() => void navigator.clipboard?.writeText(source)}
      >
        Copy diagram source
      </button>
    </figure>
  );
}

function MermaidSvg({
  diagram,
}: {
  diagram: Extract<MermaidDiagram, { ok: true }>;
}) {
  const placed = layout(diagram);
  const width = Math.max(...placed.map((node) => node.x + 130), 160);
  const height = Math.max(...placed.map((node) => node.y + 50), 80);
  const byId = new Map(placed.map((node) => [node.id, node]));
  return (
    <svg
      role="img"
      aria-label="Mermaid diagram"
      viewBox={`0 0 ${width} ${height}`}
      width="100%"
      height={Math.min(height, 280)}
    >
      {diagram.edges.map((edge, index) => {
        const from = byId.get(edge.from);
        const to = byId.get(edge.to);
        if (!from || !to) return null;
        return (
          <g key={`${edge.from}:${edge.to}:${index}`}>
            <line
              x1={from.x + 60}
              y1={from.y + 20}
              x2={to.x + 60}
              y2={to.y + 20}
              stroke="currentColor"
            />
            {edge.label ? (
              <text
                x={(from.x + to.x) / 2 + 60}
                y={(from.y + to.y) / 2 + 16}
                fill="currentColor"
                fontSize="11"
              >
                {edge.label}
              </text>
            ) : null}
          </g>
        );
      })}
      {placed.map((node) =>
        node.shape === "diamond" ? (
          <polygon
            key={node.id}
            points={`${node.x + 60},${node.y} ${node.x + 120},${node.y + 20} ${node.x + 60},${node.y + 40} ${node.x},${node.y + 20}`}
            fill="var(--surface-raised)"
            stroke="currentColor"
          />
        ) : (
          <rect
            key={node.id}
            x={node.x}
            y={node.y}
            width={120}
            height={40}
            rx={node.shape === "round" ? 16 : 6}
            fill="var(--surface-raised)"
            stroke="currentColor"
          />
        ),
      )}
      {placed.map((node) => (
        <text
          key={`${node.id}:label`}
          x={node.x + 60}
          y={node.y + 24}
          textAnchor="middle"
          fill="currentColor"
          fontSize="12"
        >
          {node.label}
        </text>
      ))}
    </svg>
  );
}

function SequenceSvg({
  diagram,
}: {
  diagram: Extract<MermaidSource, { kind: "sequence" }>;
}) {
  const column = 120;
  const width = Math.max(diagram.actors.length * column, 160);
  const height = 56 + diagram.messages.length * 36;
  return (
    <svg
      role="img"
      aria-label="Mermaid sequence diagram"
      viewBox={`0 0 ${width} ${height}`}
      width="100%"
      height={Math.min(height, 280)}
    >
      {diagram.actors.map((actor, index) => {
        const x = index * column + 60;
        return (
          <g key={actor}>
            <rect
              x={x - 40}
              y={8}
              width={80}
              height={28}
              rx={6}
              fill="var(--surface-raised)"
              stroke="currentColor"
            />
            <text
              x={x}
              y={26}
              textAnchor="middle"
              fill="currentColor"
              fontSize="12"
            >
              {actor}
            </text>
            <line
              x1={x}
              y1={36}
              x2={x}
              y2={height - 8}
              stroke="currentColor"
              strokeDasharray="3 4"
            />
          </g>
        );
      })}
      {diagram.messages.map((message, index) => {
        const from = diagram.actors.indexOf(message.from);
        const to = diagram.actors.indexOf(message.to);
        if (from < 0 || to < 0) return null;
        const y = 56 + index * 36;
        const x1 = from * column + 60;
        const x2 = to * column + 60;
        return (
          <g key={`${message.from}:${message.to}:${index}`}>
            <line
              x1={x1}
              y1={y}
              x2={x2}
              y2={y}
              stroke="currentColor"
              strokeDasharray={message.dashed ? "4 4" : undefined}
            />
            {message.label ? (
              <text
                x={(x1 + x2) / 2}
                y={y - 6}
                textAnchor="middle"
                fill="currentColor"
                fontSize="11"
              >
                {message.label}
              </text>
            ) : null}
          </g>
        );
      })}
    </svg>
  );
}

function ClassSvg({
  diagram,
}: {
  diagram: Extract<MermaidSource, { kind: "class" }>;
}) {
  const column = 150;
  const width = Math.max(diagram.types.length * column, 160);
  const height = 72 + Math.max(diagram.relations.length, 1) * 36;
  return (
    <svg
      role="img"
      aria-label="Mermaid class diagram"
      viewBox={`0 0 ${width} ${height}`}
      width="100%"
      height={Math.min(height, 280)}
    >
      {diagram.types.map((type, index) => {
        const x = index * column + 12;
        return (
          <g key={type.id}>
            <rect
              x={x}
              y={8}
              width={126}
              height={36 + type.members.length * 14}
              rx={6}
              fill="var(--surface-raised)"
              stroke="currentColor"
            />
            <text
              x={x + 63}
              y={24}
              textAnchor="middle"
              fill="currentColor"
              fontSize="12"
            >
              {type.id}
            </text>
            {type.members.map((member, memberIndex) => (
              <text
                key={`${type.id}:${memberIndex}`}
                x={x + 8}
                y={42 + memberIndex * 14}
                fill="currentColor"
                fontSize="10"
              >
                {member}
              </text>
            ))}
          </g>
        );
      })}
      {diagram.relations.map((relation, index) => {
        const from = diagram.types.findIndex(
          (type) => type.id === relation.from,
        );
        const to = diagram.types.findIndex((type) => type.id === relation.to);
        if (from < 0 || to < 0) return null;
        const y = 72 + index * 28;
        const x1 = from * column + 75;
        const x2 = to * column + 75;
        return (
          <g key={`${relation.from}:${relation.to}:${index}`}>
            <line
              x1={x1}
              y1={y}
              x2={x2}
              y2={y}
              stroke="currentColor"
              strokeDasharray={
                relation.kind === "aggregation" ? "4 4" : undefined
              }
            />
            <text
              x={(x1 + x2) / 2}
              y={y - 6}
              textAnchor="middle"
              fill="currentColor"
              fontSize="10"
            >
              {relation.kind === "inheritance"
                ? "extends"
                : relation.kind === "composition"
                  ? "owns"
                  : relation.kind === "aggregation"
                    ? "has"
                    : ""}
            </text>
          </g>
        );
      })}
    </svg>
  );
}

function layout(diagram: Extract<MermaidDiagram, { ok: true }>) {
  const outgoing = new Map<string, string[]>();
  const incoming = new Map<string, number>();
  for (const node of diagram.nodes) {
    outgoing.set(node.id, []);
    incoming.set(node.id, 0);
  }
  for (const edge of diagram.edges) {
    outgoing.get(edge.from)?.push(edge.to);
    incoming.set(edge.to, (incoming.get(edge.to) ?? 0) + 1);
  }
  const layer = new Map<string, number>();
  const queue = diagram.nodes
    .filter((node) => (incoming.get(node.id) ?? 0) === 0)
    .map((node) => node.id);
  if (queue.length === 0 && diagram.nodes[0]) {
    queue.push(diagram.nodes[0].id);
  }
  while (queue.length > 0) {
    const id = queue.shift();
    if (!id || layer.has(id)) continue;
    const parentLayer = Math.max(
      0,
      ...diagram.edges
        .filter((edge) => edge.to === id && layer.has(edge.from))
        .map((edge) => (layer.get(edge.from) ?? 0) + 1),
    );
    layer.set(id, parentLayer);
    for (const next of outgoing.get(id) ?? []) {
      queue.push(next);
    }
  }
  const columns = new Map<number, string[]>();
  for (const node of diagram.nodes) {
    const depth = layer.get(node.id) ?? 0;
    const column = columns.get(depth) ?? [];
    column.push(node.id);
    columns.set(depth, column);
  }
  return diagram.nodes.map((node) => {
    const depth = layer.get(node.id) ?? 0;
    const index = (columns.get(depth) ?? []).indexOf(node.id);
    return diagram.direction === "LR"
      ? { ...node, x: depth * 160, y: index * 64 }
      : { ...node, x: index * 160, y: depth * 64 };
  });
}

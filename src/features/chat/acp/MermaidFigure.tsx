import { useId, useMemo, useState, type ReactNode } from "react";
import {
  parseMermaidSource,
  type MermaidDiagram,
  type MermaidSource,
} from "./mermaidDiagram";

export function MermaidFigure({ source }: { source: string }) {
  const diagram = parseMermaidSource(source);
  const identity = useMemo(() => ({ source }), [source]);
  const [copyState, setCopyState] = useState<{
    identity: typeof identity;
    status: string;
  } | null>(null);
  const status = copyState?.identity === identity ? copyState.status : "";
  return (
    <figure className="acp-mermaid">
      {diagram.ok ? (
        diagram.kind === "flowchart" ? (
          <MermaidSvg diagram={diagram} />
        ) : (
          <LaneSvg diagram={diagram} />
        )
      ) : (
        <p className="acp-mermaid__fallback">
          Diagram source is kept for copy; this mermaid subset could not be
          drawn.
        </p>
      )}
      <button
        type="button"
        className="acp-mermaid__copy"
        onClick={async () => {
          setCopyState({ identity, status: "" });
          let status = "Copied";
          try {
            await navigator.clipboard.writeText(source);
          } catch {
            status = "Copy failed";
          }
          setCopyState((current) =>
            current?.identity === identity ? { identity, status } : current,
          );
        }}
      >
        Copy diagram source
      </button>
      <span role="status">{status}</span>
    </figure>
  );
}

function DiagramSvg({
  width,
  height,
  label,
  children,
}: {
  width: number;
  height: number;
  label: string;
  children: ReactNode;
}) {
  return (
    <svg
      role="img"
      aria-label={`Mermaid ${label}`}
      viewBox={`0 0 ${width} ${height}`}
      width="100%"
      height={height}
    >
      {children}
    </svg>
  );
}

function MermaidSvg({
  diagram,
}: {
  diagram: Extract<MermaidDiagram, { ok: true }>;
}) {
  const placed = layout(diagram);
  const marker = useId();
  const width = Math.max(...placed.map((node) => node.x + 300));
  const height = Math.max(...placed.map((node) => node.y + 80));
  const byId = new Map(placed.map((node) => [node.id, node]));
  return (
    <DiagramSvg label="diagram" width={width} height={height}>
      <defs>
        <marker
          id={marker}
          viewBox="0 0 10 10"
          refX="10"
          refY="5"
          markerWidth="7"
          markerHeight="7"
          orient="auto"
        >
          <path d="M0 0L10 5L0 10" />
        </marker>
      </defs>
      {diagram.edges.map((edge, index) => {
        const from = byId.get(edge.from)!;
        const to = byId.get(edge.to)!;
        const horizontal = diagram.direction === "LR";
        const returning = horizontal ? to.x <= from.x : to.y <= from.y;
        const startX = from.x + (horizontal ? 240 : 120);
        const startY = from.y + (horizontal ? 32 : returning ? 0 : 64);
        const endX = to.x + (horizontal ? 0 : 120);
        const endY = to.y + (horizontal ? 32 : returning ? 64 : 0);
        const rail = horizontal
          ? Math.max(from.y, to.y) + 84
          : Math.max(from.x, to.x) + 280;
        const route = returning
          ? horizontal
            ? `L${startX + 20} ${startY}L${startX + 20} ${rail}L${endX - 16} ${rail}L${endX - 16} ${endY}`
            : `L${startX} ${startY - 16}L${rail} ${startY - 16}L${rail} ${endY + 16}L${endX} ${endY + 16}`
          : "";
        return (
          <g key={index}>
            <path
              d={`M${startX} ${startY}${route}L${endX} ${endY}`}
              markerEnd={`url(#${marker})`}
            />
            {edge.label ? (
              <text
                x={returning && !horizontal ? rail : (startX + endX) / 2}
                y={returning && horizontal ? rail : (startY + endY) / 2 - 4}
              >
                {edge.label}
              </text>
            ) : null}
          </g>
        );
      })}
      {placed.map((node) => (
        <g key={node.id} transform={`translate(${node.x} ${node.y})`}>
          {node.shape === "diamond" ? (
            <polygon points="120,0 240,32 120,64 0,32" />
          ) : (
            <rect
              width={240}
              height={64}
              rx={node.shape === "round" ? 16 : 6}
            />
          )}
          <text x={120} y={36 - (node.lines.length - 1) * 8}>
            {node.lines.map((line, index) => (
              <tspan key={index} x={120} dy={index ? 16 : 0}>
                {line}
              </tspan>
            ))}
          </text>
        </g>
      ))}
    </DiagramSvg>
  );
}

function LaneSvg({
  diagram,
}: {
  diagram: Extract<MermaidSource, { kind: "sequence" | "class" | "state" }>;
}) {
  const klass = diagram.kind === "class";
  const sequence = diagram.kind === "sequence";
  const nodes: { id: string; members?: string[]; terminal?: boolean }[] = klass
    ? diagram.types
    : sequence
      ? diagram.actors.map((id) => ({ id }))
      : diagram.states;
  const edges = klass
    ? diagram.relations.map((relation) => ({
        ...relation,
        label: {
          inheritance: "extends",
          composition: "owns",
          aggregation: "has",
          association: "",
        }[relation.kind],
        dashed: relation.kind === "aggregation",
      }))
    : sequence
      ? diagram.messages
      : diagram.transitions;
  const column = klass ? 150 : 120;
  const width = Math.max(nodes.length * column, 160);
  const height =
    (sequence ? 56 : 72) + Math.max(edges.length, sequence ? 0 : 1) * 36;
  return (
    <DiagramSvg label={`${diagram.kind} diagram`} width={width} height={height}>
      {nodes.map((node, index) => {
        const center = index * column + column / 2;
        const size = klass ? 126 : sequence ? 80 : 88;
        return (
          <g key={node.id}>
            {node.terminal ? (
              <circle cx={center} cy={22} r={8} fill="currentColor" />
            ) : (
              <>
                <rect
                  x={center - size / 2}
                  y={8}
                  width={size}
                  height={klass ? 36 + (node.members?.length ?? 0) * 14 : 28}
                  rx={sequence || klass ? 6 : 14}
                />
                <text x={center} y={klass ? 24 : 26}>
                  {node.id}
                </text>
              </>
            )}
            {node.members?.map((member, memberIndex) => (
              <text
                key={memberIndex}
                x={center - 55}
                y={42 + memberIndex * 14}
                textAnchor="start"
                fontSize="10"
              >
                {member}
              </text>
            ))}
            {sequence ? (
              <line
                x1={center}
                y1={36}
                x2={center}
                y2={height - 8}
                strokeDasharray="3 4"
              />
            ) : null}
          </g>
        );
      })}
      {edges.map((edge, index) => {
        const start =
          nodes.findIndex((node) => node.id === edge.from) * column +
          column / 2;
        const end =
          nodes.findIndex((node) => node.id === edge.to) * column + column / 2;
        const row = (klass ? 72 : 56) + index * (klass ? 28 : 36);
        return (
          <g key={index}>
            <line
              x1={start}
              y1={row}
              x2={end}
              y2={row}
              strokeDasharray={
                "dashed" in edge && edge.dashed ? "4 4" : undefined
              }
            />
            {edge.label ? (
              <text
                x={(start + end) / 2}
                y={row - 6}
                fontSize={klass ? 10 : 11}
              >
                {edge.label}
              </text>
            ) : null}
          </g>
        );
      })}
    </DiagramSvg>
  );
}

function layout(diagram: Extract<MermaidDiagram, { ok: true }>) {
  const layer = new Map<string, number>();
  const queue = diagram.nodes
    .filter((node) => !diagram.edges.some((edge) => edge.to === node.id))
    .map((node) => ({ id: node.id, depth: 0 }));
  if (!queue.length) queue.push({ id: diagram.nodes[0].id, depth: 0 });
  for (let cursor = 0; cursor < queue.length; cursor++) {
    const { id, depth } = queue[cursor];
    if (layer.has(id)) continue;
    layer.set(id, depth);
    for (const edge of diagram.edges)
      if (edge.from === id && !layer.has(edge.to))
        queue.push({ id: edge.to, depth: depth + 1 });
  }
  const columns = new Map<number, number>();
  return diagram.nodes.map((node) => {
    const depth = layer.get(node.id) ?? 0;
    const index = columns.get(depth) ?? 0;
    columns.set(depth, index + 1);
    const lines =
      node.label
        .match(/\S.{0,28}(?:\s|$)|\S{1,30}/gu)
        ?.map((line) => line.trim()) ?? [];
    return diagram.direction === "LR"
      ? { ...node, lines, x: depth * 300 + 20, y: index * 104 + 20 }
      : { ...node, lines, x: index * 320 + 20, y: depth * 104 + 20 };
  });
}

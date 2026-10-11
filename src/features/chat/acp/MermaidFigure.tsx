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
    <figure
      className="acp-mermaid"
      tabIndex={diagram.ok ? 0 : undefined}
      aria-label="Diagram"
    >
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
  left = 0,
  width,
  height,
  label,
  children,
}: {
  left?: number;
  width: number;
  height: number;
  label: string;
  children: ReactNode;
}) {
  return (
    <svg
      role="img"
      aria-label={`Mermaid ${label}`}
      viewBox={`${left} 0 ${width} ${height}`}
      width={width}
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
  const byId = new Map(placed.map((node) => [node.id, node]));
  const edges = diagram.edges.map((edge) => {
    const from = byId.get(edge.from)!;
    const to = byId.get(edge.to)!;
    const horizontal = diagram.direction === "LR";
    const returning = horizontal ? to.x <= from.x : to.y <= from.y;
    const lateral = horizontal !== returning;
    const startX = from.x + (lateral ? 240 : 120);
    const startY = from.y + (lateral ? 32 : 64);
    const endX = to.x + (lateral ? (returning ? 240 : 0) : 120);
    const endY = to.y + (lateral ? 32 : returning ? 64 : 0);
    const rail = horizontal
      ? Math.max(from.y, to.y) + 76
      : Math.max(from.x, to.x) + 280;
    const route = returning
      ? horizontal
        ? `L${startX} ${rail}L${endX} ${rail}`
        : `L${rail} ${startY}L${rail} ${endY}`
      : "";
    return {
      label: edge.label,
      path: `M${startX} ${startY}${route}L${endX} ${endY}`,
      x: returning && !horizontal ? rail : (startX + endX) / 2,
      y: returning && horizontal ? rail : (startY + endY) / 2 - 4,
    };
  });
  const labelledEdges = edges.filter((edge) => edge.label);
  const bounds = [
    ...placed.map((node) => ({
      x: node.x + 120,
      half: Math.max(0, ...node.lines.map((line) => line.length)) * 7,
    })),
    ...labelledEdges.map((edge) => ({
      x: edge.x,
      half: edge.label.length * 7,
    })),
  ];
  const left = Math.min(0, ...bounds.map((bound) => bound.x - bound.half - 8));
  const width =
    Math.max(
      ...placed.map((node) => node.x + 300),
      ...bounds.map((bound) => bound.x + bound.half + 8),
    ) - left;
  const height = Math.max(
    ...placed.map((node) => node.y + 80),
    ...labelledEdges.map((edge) => edge.y + 8),
  );
  return (
    <DiagramSvg label="diagram" left={left} width={width} height={height}>
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
      {edges.map((edge, index) => (
        <g key={index}>
          <path d={edge.path} markerEnd={`url(#${marker})`} />
          {edge.label ? (
            <text x={edge.x} y={edge.y}>
              {edge.label}
            </text>
          ) : null}
        </g>
      ))}
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
  const labels = [
    ...nodes.flatMap((node) => [node.id, ...(node.members ?? [])]),
    ...edges.map((edge) => edge.label),
  ];
  const column = Math.max(
    klass ? 150 : 120,
    ...labels.map((label) => label.length * 14 + 48),
  );
  const rowStart = klass
    ? 72 + Math.max(...nodes.map((node) => node.members?.length ?? 0)) * 16
    : 56;
  const width = Math.max(nodes.length * column, 160);
  const height =
    (klass ? rowStart : sequence ? 56 : 72) +
    Math.max(edges.length, sequence ? 0 : 1) * 36;
  return (
    <DiagramSvg label={`${diagram.kind} diagram`} width={width} height={height}>
      {nodes.map((node, index) => {
        const center = index * column + column / 2;
        const size = klass
          ? column - 24
          : Math.max(sequence ? 80 : 88, node.id.length * 14 + 24);
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
                  height={klass ? 36 + (node.members?.length ?? 0) * 16 : 28}
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
                x={center - size / 2 + 12}
                y={42 + memberIndex * 16}
                textAnchor="start"
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
        const row = rowStart + index * 36;
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
              <text x={(start + end) / 2} y={row - 6}>
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

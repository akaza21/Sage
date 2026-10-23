import {
  useId,
  useMemo,
  useState,
  type CSSProperties,
  type ReactNode,
} from "react";
import { Copy } from "../../../ui/leanIcons";
import {
  parseMermaidSource,
  type MermaidDiagram,
  type MermaidSource,
} from "./mermaidDiagram";
import {
  FLOWCHART_LABEL_LINE_HEIGHT,
  flowchartReturnRail,
  layoutFlowchart,
} from "./mermaidFlowchartLayout";
import {
  MERMAID_RELATION_LABELS,
  mermaidAccessibility,
} from "./mermaidAccessibility";

export function MermaidFigure({
  source,
  copySource = true,
}: {
  source: string;
  copySource?: boolean;
}) {
  const figureId = useId();
  const diagram = parseMermaidSource(source);
  const accessible = mermaidAccessibility(diagram, source);
  const identity = useMemo(() => ({ source }), [source]);
  const [copyState, setCopyState] = useState<{
    identity: typeof identity;
    status: string;
  } | null>(null);
  const status = copyState?.identity === identity ? copyState.status : "";
  return (
    <div className="acp-mermaid-container">
      <figure aria-label={`${accessible.name} (figure ${figureId})`}>
        <div
          className="acp-mermaid"
          role="region"
          aria-label={`${accessible.name} (figure ${figureId}) scroll area`}
          // No tabIndex: the WebView makes an overflowing scroller a keyboard
          // stop by itself, so a diagram that fits never adds a Tab stop.
        >
          {diagram.ok ? (
            diagram.kind === "flowchart" ? (
              <MermaidSvg
                diagram={diagram}
                description={accessible.description}
              />
            ) : (
              <LaneSvg diagram={diagram} description={accessible.description} />
            )
          ) : (
            <p className="acp-mermaid__fallback">
              Diagram source is kept for copy; this mermaid subset could not be
              drawn.
            </p>
          )}
        </div>
      </figure>
      {copySource || !diagram.ok ? (
        <button
          type="button"
          className="icon-button acp-mermaid__copy"
          aria-label="Copy diagram source"
          title="Copy diagram source"
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
          <Copy aria-hidden="true" />
        </button>
      ) : null}
      <span role="status">{status}</span>
    </div>
  );
}

function DiagramSvg({
  left = 0,
  width,
  height,
  label,
  description,
  children,
}: {
  left?: number;
  width: number;
  height: number;
  label: string;
  description: string;
  children: ReactNode;
}) {
  const descriptionId = useId();
  return (
    <svg
      role="img"
      aria-label={`Mermaid ${label}`}
      aria-describedby={descriptionId}
      viewBox={`${left} 0 ${width} ${height}`}
      width={width}
      height={height}
      style={{ "--mermaid-width": `${width}px` } as CSSProperties}
    >
      <desc id={descriptionId}>{description}</desc>
      {children}
    </svg>
  );
}

function MermaidSvg({
  diagram,
  description,
}: {
  diagram: Extract<MermaidDiagram, { ok: true }>;
  description: string;
}) {
  const placed = layoutFlowchart(diagram);
  const marker = useId();
  const byId = new Map(placed.map((node) => [node.id, node]));
  const edges = diagram.edges.map((edge) => {
    const from = byId.get(edge.from)!;
    const to = byId.get(edge.to)!;
    const horizontal = diagram.direction === "LR";
    const returning = horizontal ? to.x <= from.x : to.y <= from.y;
    const lateral = horizontal !== returning;
    const startX = from.x + (lateral ? from.width : from.width / 2);
    const startY = from.y + (lateral ? from.height / 2 : from.height);
    const endX = to.x + (lateral ? (returning ? to.width : 0) : to.width / 2);
    const endY = to.y + (lateral ? to.height / 2 : returning ? to.height : 0);
    const rail = returning
      ? flowchartReturnRail(placed, from, to, diagram.direction)
      : 0;
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
  const bounds = labelledEdges.map((edge) => ({
    x: edge.x,
    half: edge.label.length * 7,
  }));
  const left = Math.min(0, ...bounds.map((bound) => bound.x - bound.half - 8));
  const width =
    Math.max(
      ...placed.map((node) => node.x + node.width + 60),
      ...bounds.map((bound) => bound.x + bound.half + 8),
    ) - left;
  const height = Math.max(
    ...placed.map((node) => node.y + node.height + 16),
    ...labelledEdges.map((edge) => edge.y + 8),
  );
  return (
    <DiagramSvg
      label="diagram"
      left={left}
      width={width}
      height={height}
      description={description}
    >
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
            <polygon
              points={`${node.width / 2},0 ${node.width},${node.height / 2} ${node.width / 2},${node.height} 0,${node.height / 2}`}
            />
          ) : (
            <rect
              width={node.width}
              height={node.height}
              rx={node.shape === "round" ? 16 : 6}
            />
          )}
          <text
            x={node.width / 2}
            y={
              node.height / 2 +
              4 -
              ((node.lines.length - 1) * FLOWCHART_LABEL_LINE_HEIGHT) / 2
            }
          >
            {node.lines.map((line, index) => (
              <tspan
                key={index}
                x={node.width / 2}
                dy={index ? FLOWCHART_LABEL_LINE_HEIGHT : 0}
              >
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
  description,
}: {
  diagram: Extract<MermaidSource, { kind: "sequence" | "class" | "state" }>;
  description: string;
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
        label: MERMAID_RELATION_LABELS[relation.kind],
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
    <DiagramSvg
      label={`${diagram.kind} diagram`}
      width={width}
      height={height}
      description={description}
    >
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

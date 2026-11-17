import type {
  MermaidDiagram,
  MermaidDirection,
  MermaidNode,
} from "./mermaidDiagram";

const BODY_GLYPH_WIDTH = 14;
export const FLOWCHART_LABEL_LINE_HEIGHT = 16;
const MAX_NODE_WIDTH = 320;

function sizeNode(node: MermaidNode) {
  const shapeScale = node.shape === "diamond" ? 2 : 1;
  const characters = Array.from(node.label);
  const width = Math.min(
    MAX_NODE_WIDTH,
    Math.max(240, characters.length * BODY_GLYPH_WIDTH * shapeScale + 32),
  );
  const columns = Math.floor((width - 32) / shapeScale / BODY_GLYPH_WIDTH);
  const lines: string[] = [];
  let remaining = characters;
  while (remaining.length > columns) {
    const space = remaining.lastIndexOf(" ", columns);
    const end = space > 0 ? space : columns;
    lines.push(remaining.slice(0, end).join(""));
    remaining = remaining.slice(end);
    if (remaining[0] === " ") remaining = remaining.slice(1);
  }
  lines.push(remaining.join(""));
  return {
    ...node,
    lines,
    width:
      node.shape === "diamond"
        ? Math.max(
            240,
            Math.max(...lines.map((line) => Array.from(line).length)) *
              BODY_GLYPH_WIDTH *
              shapeScale +
              32,
          )
        : width,
    height: Math.max(
      64,
      lines.length * FLOWCHART_LABEL_LINE_HEIGHT * shapeScale + 24,
    ),
  };
}

type FlowchartNode = ReturnType<typeof sizeNode> & { x: number; y: number };
type Point = readonly [x: number, y: number];

export interface FlowchartEdgeRoute {
  label: string;
  points: Point[];
  labelAt: { x: number; y: number };
}

const isHorizontal = (direction: MermaidDirection) =>
  direction === "LR" || direction === "RL";

function returnRail(
  nodes: readonly FlowchartNode[],
  from: FlowchartNode,
  to: FlowchartNode,
  horizontal: boolean,
) {
  const axis = horizontal ? "x" : "y";
  const first = Math.min(from[axis], to[axis]);
  const last = Math.max(from[axis], to[axis]);
  const extent = nodes.reduce((maximum, node) => {
    if (node[axis] < first || node[axis] > last) return maximum;
    return Math.max(
      maximum,
      horizontal ? node.y + node.height : node.x + node.width,
    );
  }, 0);
  return extent + (horizontal ? 12 : 40);
}

function placeForward(
  diagram: Extract<MermaidDiagram, { ok: true }>,
  horizontal: boolean,
): FlowchartNode[] {
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
  const sized = diagram.nodes.map(sizeNode);
  const siblingSize = Math.max(
    ...sized.map((node) => (horizontal ? node.height : node.width)),
  );
  const layerSizes = new Map<number, number>();
  for (const node of sized) {
    const depth = layer.get(node.id) ?? 0;
    layerSizes.set(
      depth,
      Math.max(
        layerSizes.get(depth) ?? 0,
        horizontal ? node.width : node.height,
      ),
    );
  }
  const offsets = new Map<number, number>();
  let offset = 20;
  for (const [depth, size] of [...layerSizes].sort(
    ([first], [second]) => first - second,
  )) {
    offsets.set(depth, offset);
    offset += size + (horizontal ? 60 : 40);
  }
  const siblings = new Map<number, number>();
  return sized.map((node) => {
    const depth = layer.get(node.id) ?? 0;
    const siblingOffset = siblings.get(depth) ?? 20;
    siblings.set(depth, siblingOffset + siblingSize + (horizontal ? 40 : 80));
    const inset = (siblingSize - (horizontal ? node.height : node.width)) / 2;
    return {
      ...node,
      x: horizontal ? offsets.get(depth)! : siblingOffset + inset,
      y: horizontal ? siblingOffset + inset : offsets.get(depth)!,
    };
  });
}

function routeForward(
  nodes: readonly FlowchartNode[],
  diagram: Extract<MermaidDiagram, { ok: true }>,
  horizontal: boolean,
): FlowchartEdgeRoute[] {
  const byId = new Map(nodes.map((node) => [node.id, node]));
  return diagram.edges.map((edge) => {
    const from = byId.get(edge.from)!;
    const to = byId.get(edge.to)!;
    const returning = horizontal ? to.x <= from.x : to.y <= from.y;
    const lateral = horizontal !== returning;
    const start: Point = [
      from.x + (lateral ? from.width : from.width / 2),
      from.y + (lateral ? from.height / 2 : from.height),
    ];
    const end: Point = [
      to.x + (lateral ? (returning ? to.width : 0) : to.width / 2),
      to.y + (lateral ? to.height / 2 : returning ? to.height : 0),
    ];
    if (!returning)
      return {
        label: edge.label,
        points: [start, end],
        labelAt: {
          x: (start[0] + end[0]) / 2,
          y: (start[1] + end[1]) / 2 - 4,
        },
      };
    const rail = returnRail(nodes, from, to, horizontal);
    return {
      label: edge.label,
      points: horizontal
        ? [start, [start[0], rail], [end[0], rail], end]
        : [start, [rail, start[1]], [rail, end[1]], end],
      labelAt: horizontal
        ? { x: (start[0] + end[0]) / 2, y: rail }
        : { x: rail, y: (start[1] + end[1]) / 2 - 4 },
    };
  });
}

/**
 * Lays the chart out top-down or left-right, then mirrors the main axis for
 * BT and RL so edge routing only has to handle the forward orientations.
 */
export function layoutFlowchart(
  diagram: Extract<MermaidDiagram, { ok: true }>,
) {
  const horizontal = isHorizontal(diagram.direction);
  const forward = placeForward(diagram, horizontal);
  const edges = routeForward(forward, diagram, horizontal);
  if (diagram.direction !== "BT" && diagram.direction !== "RL")
    return { nodes: forward, edges };
  const main = horizontal ? "x" : "y";
  const size = horizontal ? "width" : "height";
  const extent =
    Math.max(...forward.map((node) => node[main] + node[size])) + 20;
  const mirror = (point: Point): Point =>
    horizontal ? [extent - point[0], point[1]] : [point[0], extent - point[1]];
  return {
    nodes: forward.map((node) => ({
      ...node,
      [main]: extent - node[main] - node[size],
    })),
    edges: edges.map((edge) => {
      const [x, y] = mirror([edge.labelAt.x, edge.labelAt.y]);
      return { ...edge, points: edge.points.map(mirror), labelAt: { x, y } };
    }),
  };
}

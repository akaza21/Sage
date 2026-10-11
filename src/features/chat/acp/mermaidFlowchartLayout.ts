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
    Math.max(240, (characters.length * BODY_GLYPH_WIDTH + 32) * shapeScale),
  );
  const columns = Math.floor((width / shapeScale - 32) / BODY_GLYPH_WIDTH);
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
    width,
    height: Math.max(
      64,
      (lines.length * FLOWCHART_LABEL_LINE_HEIGHT + 24) * shapeScale,
    ),
  };
}

export function flowchartReturnRail(
  nodes: ReturnType<typeof layoutFlowchart>,
  from: ReturnType<typeof layoutFlowchart>[number],
  to: ReturnType<typeof layoutFlowchart>[number],
  direction: MermaidDirection,
) {
  const horizontal = direction === "LR";
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

export function layoutFlowchart(
  diagram: Extract<MermaidDiagram, { ok: true }>,
) {
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
  const horizontal = diagram.direction === "LR";
  const sized = diagram.nodes.map(sizeNode);
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
    siblings.set(
      depth,
      siblingOffset + (horizontal ? node.height + 40 : node.width + 80),
    );
    return {
      ...node,
      x: horizontal ? offsets.get(depth)! : siblingOffset,
      y: horizontal ? siblingOffset : offsets.get(depth)!,
    };
  });
}

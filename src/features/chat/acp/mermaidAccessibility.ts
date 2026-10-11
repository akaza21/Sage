import type { MermaidSource } from "./mermaidDiagram";

export const MERMAID_RELATION_LABELS = {
  inheritance: "extends",
  composition: "owns",
  aggregation: "has",
  association: "",
};

function connection(from: string, to: string, label: string) {
  return `${from} to ${to}${label ? `: ${label}` : ""}`;
}

function describe(nodes: string[], edges: string[], details = nodes) {
  return {
    name: `Diagram: ${nodes
      .slice(0, 2)
      .map((node) => node.slice(0, 64))
      .join(", ")}`,
    description: `Nodes: ${details.join("; ")}. Connections: ${edges.join("; ")}.`,
  };
}

export function mermaidAccessibility(diagram: MermaidSource, source: string) {
  if (!diagram.ok) {
    return {
      name: `Diagram source: ${source.slice(0, 64).replace(/\s+/gu, " ").trim() || "empty"}`,
      description: "",
    };
  }
  if (diagram.kind === "flowchart") {
    const labels = new Map(diagram.nodes.map((node) => [node.id, node.label]));
    return describe(
      diagram.nodes.map((node) => node.label),
      diagram.edges.map((edge) =>
        connection(labels.get(edge.from)!, labels.get(edge.to)!, edge.label),
      ),
    );
  }
  if (diagram.kind === "class") {
    return describe(
      diagram.types.map((node) => node.id),
      diagram.relations.map((edge) =>
        connection(edge.from, edge.to, MERMAID_RELATION_LABELS[edge.kind]),
      ),
      diagram.types.map((node) => [node.id, ...node.members].join(", ")),
    );
  }
  return describe(
    diagram.kind === "sequence"
      ? diagram.actors
      : diagram.states.map((node) => node.id),
    (diagram.kind === "sequence" ? diagram.messages : diagram.transitions).map(
      (edge) => connection(edge.from, edge.to, edge.label),
    ),
  );
}

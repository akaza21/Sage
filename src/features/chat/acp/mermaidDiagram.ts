import type { MermaidStateSource } from "./mermaidState";
import { parseMermaidState } from "./mermaidState";

export const MAX_MERMAID_NODES = 32;
export const MAX_MERMAID_EDGES = 48;
export const MAX_MERMAID_LABEL = 64;
export const MAX_MERMAID_SOURCE_BYTES = 8192;

export type MermaidDirection = "TD" | "LR";

export interface MermaidNode {
  id: string;
  label: string;
  shape: "rect" | "round" | "diamond";
}

export interface MermaidEdge {
  from: string;
  to: string;
  label: string;
}

export const MAX_SEQUENCE_ACTORS = 16;
export const MAX_SEQUENCE_MESSAGES = 32;

export interface MermaidSequenceMessage {
  from: string;
  to: string;
  label: string;
  dashed: boolean;
}

export type MermaidDiagram =
  | {
      ok: true;
      direction: MermaidDirection;
      nodes: MermaidNode[];
      edges: MermaidEdge[];
    }
  | { ok: false; reason: "unsupported" | "limit" };

export const MAX_CLASS_TYPES = 12;
export const MAX_CLASS_RELATIONS = 24;
export const MAX_CLASS_MEMBERS = 6;

export type MermaidClassRelationKind =
  "inheritance" | "composition" | "aggregation" | "association";

export interface MermaidClassType {
  id: string;
  members: string[];
}

export interface MermaidClassRelation {
  from: string;
  to: string;
  kind: MermaidClassRelationKind;
}

export type MermaidSource =
  | (Extract<MermaidDiagram, { ok: true }> & { kind: "flowchart" })
  | {
      ok: true;
      kind: "sequence";
      actors: string[];
      messages: MermaidSequenceMessage[];
    }
  | {
      ok: true;
      kind: "class";
      types: MermaidClassType[];
      relations: MermaidClassRelation[];
    }
  | Extract<MermaidStateSource, { ok: true }>
  | { ok: false; reason: "unsupported" | "limit" };

export function parseMermaidSource(source: string): MermaidSource {
  const flowchart = parseMermaidFlowchart(source);
  if (flowchart.ok) {
    return { kind: "flowchart", ...flowchart };
  }
  const sequence = parseMermaidSequence(source);
  if (sequence.ok) {
    return sequence;
  }
  if (flowchart.reason === "limit" || sequence.reason === "limit") {
    return flowchart.reason === "limit" ? flowchart : sequence;
  }
  const klass = parseMermaidClass(source);
  if (klass.ok || klass.reason === "limit") {
    return klass;
  }
  return parseMermaidState(source);
}

const nodeToken =
  /^([A-Za-z][A-Za-z0-9_-]*)(?:\[([^\]]{1,64})\]|\(([^)]{1,64})\)|\{([^}]{1,64})\})?/;

export function parseMermaidFlowchart(source: string): MermaidDiagram {
  if (new TextEncoder().encode(source).length > MAX_MERMAID_SOURCE_BYTES) {
    return { ok: false, reason: "limit" };
  }
  const lines = source
    .replace(/\r\n/gu, "\n")
    .split("\n")
    .map((line) => line.replace(/\s*%%.*$/u, "").trim())
    .filter((line) => line.length > 0);
  if (lines.length === 0) {
    return { ok: false, reason: "unsupported" };
  }
  const header = /^(?:graph|flowchart)\s+(TD|TB|LR|RL)\b/iu.exec(lines[0]);
  if (!header) {
    return { ok: false, reason: "unsupported" };
  }
  const direction: MermaidDirection =
    header[1].toUpperCase() === "LR" || header[1].toUpperCase() === "RL"
      ? "LR"
      : "TD";
  const nodes = new Map<string, MermaidNode>();
  const edges: MermaidEdge[] = [];
  for (const line of lines.slice(1)) {
    const parsed = parseStatement(line);
    if (!parsed) {
      return { ok: false, reason: "unsupported" };
    }
    for (const node of parsed.nodes) {
      if (!nodes.has(node.id) && nodes.size >= MAX_MERMAID_NODES) {
        return { ok: false, reason: "limit" };
      }
      const existing = nodes.get(node.id);
      if (existing) {
        if (node.label !== node.id) {
          nodes.set(node.id, node);
        }
      } else {
        nodes.set(node.id, node);
      }
    }
    for (const edge of parsed.edges) {
      if (edges.length >= MAX_MERMAID_EDGES) {
        return { ok: false, reason: "limit" };
      }
      edges.push(edge);
    }
  }
  if (nodes.size === 0) {
    return { ok: false, reason: "unsupported" };
  }
  return {
    ok: true,
    direction,
    nodes: [...nodes.values()],
    edges,
  };
}

export function parseMermaidSequence(source: string): MermaidSource {
  if (new TextEncoder().encode(source).length > MAX_MERMAID_SOURCE_BYTES) {
    return { ok: false, reason: "limit" };
  }
  const lines = source
    .replace(/\r\n/gu, "\n")
    .split("\n")
    .map((line) => line.replace(/\s*%%.*$/u, "").trim())
    .filter((line) => line.length > 0);
  if (lines.length === 0 || !/^sequenceDiagram\b/iu.test(lines[0])) {
    return { ok: false, reason: "unsupported" };
  }
  const actors: string[] = [];
  const messages: MermaidSequenceMessage[] = [];
  for (const line of lines.slice(1)) {
    const participant = /^participant\s+([A-Za-z][A-Za-z0-9_]*)$/u.exec(line);
    if (participant) {
      if (!admitActor(actors, participant[1])) {
        return { ok: false, reason: "limit" };
      }
      continue;
    }
    const message =
      /^([A-Za-z][A-Za-z0-9_]*)\s*(-->>|->>|-->|->)\s*([A-Za-z][A-Za-z0-9_]*)\s*(?::\s*(.*))?$/u.exec(
        line,
      );
    if (!message) {
      return { ok: false, reason: "unsupported" };
    }
    if (messages.length >= MAX_SEQUENCE_MESSAGES) {
      return { ok: false, reason: "limit" };
    }
    if (!admitActor(actors, message[1]) || !admitActor(actors, message[3])) {
      return { ok: false, reason: "limit" };
    }
    const label = clampLabel(message[4] ?? "");
    if (
      label.includes("<") ||
      label.includes(">") ||
      /javascript:/iu.test(label)
    ) {
      return { ok: false, reason: "unsupported" };
    }
    messages.push({
      from: message[1],
      to: message[3],
      label,
      dashed: message[2].startsWith("--"),
    });
  }
  if (actors.length === 0 || messages.length === 0) {
    return { ok: false, reason: "unsupported" };
  }
  return { ok: true, kind: "sequence", actors, messages };
}

const classRelationToken =
  /^([A-Za-z][A-Za-z0-9_]*)\s*(<\|--|\*--|o--|-->)\s*([A-Za-z][A-Za-z0-9_]*)$/u;

export function parseMermaidClass(source: string): MermaidSource {
  if (new TextEncoder().encode(source).length > MAX_MERMAID_SOURCE_BYTES) {
    return { ok: false, reason: "limit" };
  }
  const lines = source
    .replace(/\r\n/gu, "\n")
    .split("\n")
    .map((line) => line.replace(/\s*%%.*$/u, "").trim())
    .filter((line) => line.length > 0);
  if (lines.length === 0 || !/^classDiagram\b/iu.test(lines[0])) {
    return { ok: false, reason: "unsupported" };
  }
  const types: MermaidClassType[] = [];
  const relations: MermaidClassRelation[] = [];
  for (const line of lines.slice(1)) {
    const declared = /^class\s+([A-Za-z][A-Za-z0-9_]*)$/u.exec(line);
    if (declared) {
      if (!admitClassType(types, declared[1])) {
        return { ok: false, reason: "limit" };
      }
      continue;
    }
    const member = /^([A-Za-z][A-Za-z0-9_]*)\s*:\s*(.+)$/u.exec(line);
    if (member) {
      if (!admitClassType(types, member[1])) {
        return { ok: false, reason: "limit" };
      }
      const label = clampLabel(member[2] ?? "");
      if (
        label.length === 0 ||
        label.includes("<") ||
        label.includes(">") ||
        /javascript:/iu.test(label)
      ) {
        return { ok: false, reason: "unsupported" };
      }
      const target = types.find((type) => type.id === member[1]);
      if (!target || target.members.length >= MAX_CLASS_MEMBERS) {
        return { ok: false, reason: "limit" };
      }
      target.members.push(label);
      continue;
    }
    const relation = classRelationToken.exec(line);
    if (!relation) {
      return { ok: false, reason: "unsupported" };
    }
    if (relations.length >= MAX_CLASS_RELATIONS) {
      return { ok: false, reason: "limit" };
    }
    if (
      !admitClassType(types, relation[1]) ||
      !admitClassType(types, relation[3])
    ) {
      return { ok: false, reason: "limit" };
    }
    relations.push({
      from: relation[1],
      to: relation[3],
      kind:
        relation[2] === "<|--"
          ? "inheritance"
          : relation[2] === "*--"
            ? "composition"
            : relation[2] === "o--"
              ? "aggregation"
              : "association",
    });
  }
  if (types.length === 0) {
    return { ok: false, reason: "unsupported" };
  }
  return { ok: true, kind: "class", types, relations };
}

function admitClassType(types: MermaidClassType[], id: string): boolean {
  if (types.some((type) => type.id === id)) {
    return true;
  }
  if (types.length >= MAX_CLASS_TYPES) {
    return false;
  }
  types.push({ id, members: [] });
  return true;
}

function admitActor(actors: string[], name: string): boolean {
  if (actors.includes(name)) {
    return true;
  }
  if (actors.length >= MAX_SEQUENCE_ACTORS) {
    return false;
  }
  actors.push(name);
  return true;
}

function parseStatement(line: string): {
  nodes: MermaidNode[];
  edges: MermaidEdge[];
} | null {
  const left = takeNode(line, 0);
  if (!left) {
    return null;
  }
  const rest = line.slice(left.next).trim();
  if (rest.length === 0) {
    return { nodes: [left.node], edges: [] };
  }
  const arrow = /^(-->|---)(?:\|([^|]{1,64})\|)?\s*/u.exec(rest);
  if (!arrow) {
    return null;
  }
  const right = takeNode(rest.slice(arrow[0].length), 0);
  if (!right || right.next !== rest.slice(arrow[0].length).length) {
    return null;
  }
  return {
    nodes: [left.node, right.node],
    edges: [
      {
        from: left.node.id,
        to: right.node.id,
        label: clampLabel(arrow[2] ?? ""),
      },
    ],
  };
}

function takeNode(
  source: string,
  start: number,
): { node: MermaidNode; next: number } | null {
  const match = nodeToken.exec(source.slice(start));
  if (!match) {
    return null;
  }
  const id = match[1];
  const label = clampLabel(match[2] ?? match[3] ?? match[4] ?? id);
  const shape: MermaidNode["shape"] = match[4]
    ? "diamond"
    : match[3]
      ? "round"
      : "rect";
  return {
    node: { id, label, shape },
    next: start + match[0].length,
  };
}

function clampLabel(value: string): string {
  const text = value.replace(/\s+/gu, " ").trim();
  return text.length <= MAX_MERMAID_LABEL
    ? text
    : text.slice(0, MAX_MERMAID_LABEL);
}

export const MAX_STATE_NODES = 12;
export const MAX_STATE_EDGES = 24;

export interface MermaidStateNode {
  id: string;
  terminal: boolean;
}

export interface MermaidStateTransition {
  from: string;
  to: string;
  label: string;
}

export type MermaidStateSource =
  | {
      ok: true;
      kind: "state";
      states: MermaidStateNode[];
      transitions: MermaidStateTransition[];
    }
  | { ok: false; reason: "unsupported" | "limit" };

const stateToken = /^(\[\*\]|[A-Za-z][A-Za-z0-9_]*)$/u;
const transitionToken =
  /^(\[\*\]|[A-Za-z][A-Za-z0-9_]*)\s*-->\s*(\[\*\]|[A-Za-z][A-Za-z0-9_]*)\s*(?::\s*(.*))?$/u;

export function parseMermaidState(source: string): MermaidStateSource {
  if (utf8ByteLength(source) > 8192) {
    return { ok: false, reason: "limit" };
  }
  const lines = mermaidLines(source);
  if (lines.length === 0 || !/^stateDiagram(?:-v2)?\b/iu.test(lines[0])) {
    return { ok: false, reason: "unsupported" };
  }
  const states: MermaidStateNode[] = [];
  const transitions: MermaidStateTransition[] = [];
  for (const line of lines.slice(1)) {
    const declared = /^state\s+([A-Za-z][A-Za-z0-9_]*)$/u.exec(line);
    if (declared) {
      if (!admitState(states, declared[1])) {
        return { ok: false, reason: "limit" };
      }
      continue;
    }
    const transition = transitionToken.exec(line);
    if (!transition) {
      return { ok: false, reason: "unsupported" };
    }
    if (transitions.length >= MAX_STATE_EDGES) {
      return { ok: false, reason: "limit" };
    }
    if (
      !admitState(states, transition[1]) ||
      !admitState(states, transition[2])
    ) {
      return { ok: false, reason: "limit" };
    }
    const label = clampMermaidLabel(transition[3] ?? "");
    if (
      label.includes("<") ||
      label.includes(">") ||
      /javascript:/iu.test(label)
    ) {
      return { ok: false, reason: "unsupported" };
    }
    transitions.push({
      from: transition[1],
      to: transition[2],
      label,
    });
  }
  if (states.length === 0) {
    return { ok: false, reason: "unsupported" };
  }
  return { ok: true, kind: "state", states, transitions };
}

function admitState(states: MermaidStateNode[], id: string): boolean {
  if (!stateToken.test(id)) {
    return false;
  }
  if (states.some((state) => state.id === id)) {
    return true;
  }
  if (states.length >= MAX_STATE_NODES) {
    return false;
  }
  states.push({ id, terminal: id === "[*]" });
  return true;
}

import { utf8ByteLength } from "../../../domain/text";

export function mermaidLines(source: string): string[] {
  return source
    .split("\n")
    .map((line) => line.replace(/\s*%%.*$/u, "").trim())
    .filter(Boolean);
}

export function clampMermaidLabel(value: string): string {
  return value.replace(/\s+/gu, " ").trim().slice(0, 64);
}

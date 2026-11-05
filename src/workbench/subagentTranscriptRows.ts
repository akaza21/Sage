import type { AcpItem } from "../domain/acp";
import type { ChildTranscript } from "../services/acpChildTranscript";

type ChildRead = Extract<ChildTranscript, { turns: unknown }>;

export type ChildReadState = {
  key: string;
  value: ChildTranscript | "error";
  /** The latest full read for this child, kept across later failures. */
  good?: ChildRead;
  stale: number;
};

/** One deduplicated run of child items without the spawn prompt bubble. */
export function childRows(read: ChildRead | undefined) {
  const seen = new Set<string>();
  const rows: AcpItem[] = [];
  for (const [turnIndex, childTurn] of (read?.turns ?? []).entries())
    for (const [index, child] of childTurn.items.entries()) {
      const prompt =
        turnIndex === 0 &&
        index === 0 &&
        child.kind === "text" &&
        child.payload.role === "user";
      if (prompt || seen.has(child.id)) continue;
      seen.add(child.id);
      rows.push(child);
    }
  return rows;
}

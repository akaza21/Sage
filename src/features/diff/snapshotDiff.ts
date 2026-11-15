import { AGENT_DIFF_SIDE_BYTES } from "../../domain/agentDiffIdentity";
import { utf8ByteLength } from "../../domain/text";
import type {
  DiffHunk,
  DiffLine,
  FileDiff,
  ResourceRef,
} from "../../domain/workbench";

export const MAX_SNAPSHOT_DISPLAY_LINES = 1200;
const MAX_ALIGNMENT_CELLS = 250_000;
const CONTEXT = 3;

function indexedLines(text: string) {
  const starts = new Uint32Array(text.length + 1);
  let count = 0;
  for (let offset = 0; offset < text.length; offset++) {
    if (offset === 0 || text[offset - 1] === "\n") starts[count++] = offset;
  }
  starts[count] = text.length;
  return {
    count,
    at: (index: number) => text.slice(starts[index], starts[index + 1]),
  };
}

function align(
  oldLines: string[],
  newLines: string[],
): ("context" | "removed" | "added")[] {
  const rows = oldLines.length + 1;
  const cols = newLines.length + 1;
  if (rows * cols > MAX_ALIGNMENT_CELLS) {
    return [
      ...oldLines.map(() => "removed" as const),
      ...newLines.map(() => "added" as const),
    ];
  }
  const lengths = new Uint16Array(rows * cols);
  // Intern once so the bounded quadratic window compares integers, not long
  // attacker-controlled strings on every cell.
  const tokens = new Map<string, number>();
  function intern(line: string) {
    const known = tokens.get(line);
    if (known !== undefined) return known;
    const id = tokens.size;
    tokens.set(line, id);
    return id;
  }
  const oldIds = oldLines.map(intern);
  const newIds = newLines.map(intern);
  for (let old = rows - 2; old >= 0; old--) {
    for (let next = cols - 2; next >= 0; next--) {
      lengths[old * cols + next] =
        oldIds[old] === newIds[next]
          ? 1 + lengths[(old + 1) * cols + next + 1]
          : Math.max(
              lengths[(old + 1) * cols + next],
              lengths[old * cols + next + 1],
            );
    }
  }
  const result: ("context" | "removed" | "added")[] = [];
  let old = 0;
  let next = 0;
  while (old < oldLines.length || next < newLines.length) {
    if (
      old < oldLines.length &&
      next < newLines.length &&
      oldIds[old] === newIds[next]
    ) {
      result.push("context");
      old++;
      next++;
    } else if (
      old < oldLines.length &&
      (next === newLines.length ||
        lengths[(old + 1) * cols + next] >= lengths[old * cols + next + 1])
    ) {
      result.push("removed");
      old++;
    } else {
      result.push("added");
      next++;
    }
  }
  return result;
}

function hunksFor(lines: DiffLine[]): DiffHunk[] {
  const ranges: { start: number; end: number }[] = [];
  for (let index = 0; index < lines.length; index++) {
    if (lines[index].kind === "context") continue;
    const start = Math.max(0, index - CONTEXT);
    const end = Math.min(lines.length, index + CONTEXT + 1);
    const last = ranges.at(-1);
    if (last && start <= last.end) last.end = end;
    else ranges.push({ start, end });
  }
  return ranges.map(({ start, end }) => hunkOf(lines.slice(start, end)));
}

function hunkOf(selected: DiffLine[]): DiffHunk {
  const old = selected.filter((line) => line.oldLine !== null);
  const next = selected.filter((line) => line.newLine !== null);
  // For pure insert/delete, the absent side starts immediately before the change.
  const oldStart = old[0]?.oldLine ?? Math.max(0, (next[0]?.newLine ?? 1) - 1);
  const newStart = next[0]?.newLine ?? Math.max(0, (old[0]?.oldLine ?? 1) - 1);
  return {
    header: `@@ -${oldStart},${old.length} +${newStart},${next.length} @@`,
    lines: selected,
  };
}

export function snapshotFileDiff(
  resource: ResourceRef,
  oldText: string,
  newText: string,
): FileDiff {
  if (
    [oldText, newText].some(
      (text) =>
        text.length > AGENT_DIFF_SIDE_BYTES ||
        utf8ByteLength(text) > AGENT_DIFF_SIDE_BYTES,
    )
  ) {
    throw new Error("Snapshot text exceeds its bounded admission.");
  }
  const result: FileDiff = {
    resource,
    hunks: [],
    binary: false,
    truncated: false,
  };
  if (oldText === newText) return result;
  const old = indexedLines(oldText);
  const next = indexedLines(newText);
  let prefix = 0;
  while (
    prefix < old.count &&
    prefix < next.count &&
    old.at(prefix) === next.at(prefix)
  )
    prefix++;
  let suffix = 0;
  while (
    suffix < old.count - prefix &&
    suffix < next.count - prefix &&
    old.at(old.count - suffix - 1) === next.at(next.count - suffix - 1)
  )
    suffix++;
  const oldCount = old.count - prefix - suffix;
  const newCount = next.count - prefix - suffix;
  const bound = MAX_SNAPSHOT_DISPLAY_LINES + 1;
  const oldMiddle = Array.from(
    { length: Math.min(oldCount, bound) },
    (_, index) => old.at(prefix + index),
  );
  const newMiddle = Array.from(
    { length: Math.min(newCount, bound) },
    (_, index) => next.at(prefix + index),
  );
  result.truncated = oldCount > bound || newCount > bound;
  const lines: DiffLine[] = [];
  let oldLine = Math.max(0, prefix - CONTEXT);
  let newLine = oldLine;
  function emit(kind: "context" | "removed" | "added", token: string) {
    lines.push({
      kind,
      oldLine: kind === "added" ? null : ++oldLine,
      newLine: kind === "removed" ? null : ++newLine,
      text: token.endsWith("\n") ? token.slice(0, -1) : token,
    });
    if (!token.endsWith("\n"))
      lines.push({
        kind: "meta",
        oldLine: null,
        newLine: null,
        text: " No newline at end of file",
      });
  }
  for (let index = oldLine; index < prefix; index++)
    emit("context", old.at(index));
  let oldIndex = 0;
  let newIndex = 0;
  for (const kind of align(oldMiddle, newMiddle)) {
    const token = kind === "added" ? newMiddle[newIndex] : oldMiddle[oldIndex];
    if (kind !== "added") oldIndex++;
    if (kind !== "removed") newIndex++;
    emit(kind, token);
  }
  // Do not attach tail context after a clipped middle; its line positions differ.
  if (!result.truncated)
    for (let index = 0; index < Math.min(CONTEXT, suffix); index++)
      emit("context", old.at(old.count - suffix + index));
  let remaining = MAX_SNAPSHOT_DISPLAY_LINES;
  for (const hunk of hunksFor(lines)) {
    if (hunk.lines.length > remaining) result.truncated = true;
    const selected = hunk.lines.slice(0, remaining);
    if (selected.length)
      result.hunks.push(
        selected.length === hunk.lines.length ? hunk : hunkOf(selected),
      );
    remaining -= selected.length;
  }
  return result;
}

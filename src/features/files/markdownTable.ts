/*
 * Bounded port of markdown-it 15.0.3's GFM table block rule (formerly
 * lib/rules_block/table.mjs, now src/rules_block/table.ts):
 * https://github.com/markdown-it/markdown-it/blob/15.0.3/src/rules_block/table.ts
 * Sage changes: scan source ranges; retain only admitted cells and rows.
 *
 * Copyright (c) 2014 Vitaly Puzrin, Alex Kocharin.
 *
 * Permission is hereby granted, free of charge, to any person
 * obtaining a copy of this software and associated documentation
 * files (the "Software"), to deal in the Software without
 * restriction, including without limitation the rights to use,
 * copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the
 * Software is furnished to do so, subject to the following
 * conditions:
 *
 * The above copyright notice and this permission notice shall be
 * included in all copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
 * EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES
 * OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
 * NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT
 * HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY,
 * WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
 * FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR
 * OTHER DEALINGS IN THE SOFTWARE.
 */
import type { StateBlock } from "markdown-it";
import {
  MAX_MARKDOWN_TABLE_COLUMNS,
  MAX_MARKDOWN_TABLE_ROWS,
} from "./markdownLimits";

// Preserve stock source consumption, including its (possibly negative)
// cumulative ragged-row expansion count, even for rows we do not display.
const MAX_AUTOCOMPLETED_CELLS = 0x10000;
const isSpace = (ch: number) => ch === 0x20 || ch === 0x09;

// String.trim's whitespace set, without slicing an omitted line or cell.
const isTrimSpace = (ch: number) =>
  isSpace(ch) ||
  (ch >= 0x0a && ch <= 0x0d) ||
  ch === 0xa0 ||
  ch === 0x1680 ||
  (ch >= 0x2000 && ch <= 0x200a) ||
  ch === 0x2028 ||
  ch === 0x2029 ||
  ch === 0x202f ||
  ch === 0x205f ||
  ch === 0x3000 ||
  ch === 0xfeff;

function lineRange(state: StateBlock, line: number): [number, number] {
  let start = state.bMarks[line] + state.tShift[line];
  let end = state.eMarks[line];
  while (start < end && isTrimSpace(state.src.charCodeAt(start))) start++;
  while (end > start && isTrimSpace(state.src.charCodeAt(end - 1))) end--;
  return [start, end];
}

function scanAlignments(state: StateBlock, line: number) {
  const src = state.src;
  const start = state.bMarks[line] + state.tShift[line];
  const end = state.eMarks[line];
  const first = src.charCodeAt(start);
  const second = src.charCodeAt(start + 1);
  const isMarker = (ch: number) => ch === 0x7c || ch === 0x2d || ch === 0x3a;
  if (start + 1 >= end || !isMarker(first)) return null;
  if (!isMarker(second) && !isSpace(second)) return null;
  if (first === 0x2d && isSpace(second)) return null;

  const aligns: string[] = [];
  let count = 0;
  let cellStart = start;
  for (let pos = start; pos <= end; pos++) {
    const ch = src.charCodeAt(pos);
    if (pos < end && ch !== 0x7c) {
      if (!isMarker(ch) && !isSpace(ch)) return null;
      continue;
    }
    let left = cellStart;
    let right = pos;
    while (left < right && isSpace(src.charCodeAt(left))) left++;
    while (right > left && isSpace(src.charCodeAt(right - 1))) right--;
    if (left === right) {
      if (cellStart !== start && pos !== end) return null;
    } else {
      const leadingColon = src.charCodeAt(left) === 0x3a;
      const trailingColon = src.charCodeAt(right - 1) === 0x3a;
      if (leadingColon) left++;
      if (trailingColon) right--;
      if (left >= right) return null;
      for (let i = left; i < right; i++)
        if (src.charCodeAt(i) !== 0x2d) return null;
      if (count < MAX_MARKDOWN_TABLE_COLUMNS)
        aligns.push(
          trailingColon
            ? leadingColon
              ? "center"
              : "right"
            : leadingColon
              ? "left"
              : "",
        );
      count++;
    }
    cellStart = pos + 1;
  }
  return { count, aligns };
}

// Stock escapedSplit removes the backslash immediately preceding a pipe,
// regardless of backslash parity. Count every cell, but materialize only the
// admitted prefix. Leading/trailing empty edge cells are not real columns.
function scanRow(src: string, start: number, end: number, limit: number) {
  const cells: string[] = [];
  let count = 0;
  let cellStart = start;
  let lastPos = start;
  let current = "";
  let hasPipe = false;
  for (let pos = start; pos <= end; pos++) {
    if (pos < end) {
      if (src.charCodeAt(pos) !== 0x7c) continue;
      hasPipe = true;
      if (pos > start && src.charCodeAt(pos - 1) === 0x5c) {
        if (count < limit) current += src.slice(lastPos, pos - 1);
        lastPos = pos;
        continue;
      }
    }
    if (!(pos === start || (pos === end && cellStart === end))) {
      if (count < limit) cells.push((current + src.slice(lastPos, pos)).trim());
      count++;
    }
    cellStart = lastPos = pos + 1;
    current = "";
  }
  return { count, cells, hasPipe };
}

export function boundedTable(
  state: StateBlock,
  startLine: number,
  endLine: number,
  silent: boolean,
): boolean {
  if (startLine + 2 > endLine) return false;
  const delimiterLine = startLine + 1;
  if (state.sCount[delimiterLine] < state.blkIndent) return false;
  if (state.sCount[delimiterLine] - state.blkIndent >= 4) return false;
  const alignment = scanAlignments(state, delimiterLine);
  if (!alignment) return false;
  if (state.sCount[startLine] - state.blkIndent >= 4) return false;
  const [start, end] = lineRange(state, startLine);
  const header = scanRow(state.src, start, end, MAX_MARKDOWN_TABLE_COLUMNS);
  const columnCount = header.count;
  if (!header.hasPipe || columnCount === 0 || columnCount !== alignment.count)
    return false;
  if (silent) return true;

  const visibleColumns = Math.min(columnCount, MAX_MARKDOWN_TABLE_COLUMNS);
  let truncated = columnCount > visibleColumns;
  const oldParentType = state.parentType;
  state.parentType = "table";
  const terminatorRules = state.md.block.ruler.getRules("blockquote");
  const table = state.push("table_open", "table", 1);
  const tableLines: [number, number] = [startLine, 0];
  table.map = tableLines;
  const thead = state.push("thead_open", "thead", 1);
  thead.map = [startLine, startLine + 1];
  const headerRow = state.push("tr_open", "tr", 1);
  headerRow.map = [startLine, startLine + 1];

  const pushCells = (cells: string[], tag: "th" | "td") => {
    for (let i = 0; i < visibleColumns; i++) {
      const cell = state.push(`${tag}_open`, tag, 1);
      if (alignment.aligns[i])
        cell.attrs = [["style", `text-align:${alignment.aligns[i]}`]];
      const inline = state.push("inline", "", 0);
      inline.content = cells[i] || "";
      inline.children = [];
      state.push(`${tag}_close`, tag, -1);
    }
  };
  pushCells(header.cells, "th");
  state.push("tr_close", "tr", -1);
  state.push("thead_close", "thead", -1);

  let tbodyLines: [number, number] | undefined;
  let autocompletedCells = 0;
  let nextLine = startLine + 2;
  for (; nextLine < endLine; nextLine++) {
    if (state.sCount[nextLine] < state.blkIndent) break;
    if (terminatorRules.some((rule) => rule(state, nextLine, endLine, true)))
      break;
    const [rowStart, rowEnd] = lineRange(state, nextLine);
    if (rowStart === rowEnd) break;
    if (state.sCount[nextLine] - state.blkIndent >= 4) break;
    const admitted = nextLine - startLine - 2 < MAX_MARKDOWN_TABLE_ROWS;
    const row = scanRow(
      state.src,
      rowStart,
      rowEnd,
      admitted ? visibleColumns : 0,
    );
    autocompletedCells += columnCount - row.count;
    if (autocompletedCells > MAX_AUTOCOMPLETED_CELLS) break;
    truncated ||= !admitted || row.count > MAX_MARKDOWN_TABLE_COLUMNS;
    if (!admitted) continue;

    if (nextLine === startLine + 2) {
      const tbody = state.push("tbody_open", "tbody", 1);
      tbody.map = tbodyLines = [startLine + 2, 0];
    }
    const tr = state.push("tr_open", "tr", 1);
    tr.map = [nextLine, nextLine + 1];
    pushCells(row.cells, "td");
    state.push("tr_close", "tr", -1);
  }
  if (tbodyLines) {
    state.push("tbody_close", "tbody", -1);
    tbodyLines[1] = nextLine;
  }
  state.push("table_close", "table", -1);
  tableLines[1] = nextLine;
  if (truncated) {
    state.env.sageTableTruncated = true;
    table.meta = { truncated: true };
  }
  state.parentType = oldParentType;
  state.line = nextLine;
  return true;
}

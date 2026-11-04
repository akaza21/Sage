/**
 * Line-level Myers diff for the gutter (EP-14, AD-7). It runs in the gutter
 * worker, never on the main thread. The linear-space middle-snake search is
 * GNU diff's, without its heuristics except a cost cap: past the cap a split
 * is taken at the middle of both ranges, which keeps the result a valid
 * (if longer) edit script and the work bounded on unrelated texts.
 */

/** Half-open, 0-based line ranges: `[start, end)` now, `[baseStart, baseEnd)` staged. */
export interface LineChange {
  start: number;
  end: number;
  baseStart: number;
  baseEnd: number;
}

/** Diagonals searched per split before the cap gives up on a shortest script. */
const MAX_SNAKE_COST = 256;

export function diffLines(
  base: readonly string[],
  current: readonly string[],
): LineChange[] {
  let prefix = 0;
  const shortest = Math.min(base.length, current.length);
  while (prefix < shortest && base[prefix] === current[prefix]) prefix += 1;
  let suffix = 0;
  while (
    suffix < shortest - prefix &&
    base[base.length - 1 - suffix] === current[current.length - 1 - suffix]
  )
    suffix += 1;
  const n = base.length - prefix - suffix;
  const m = current.length - prefix - suffix;
  if (n === 0 && m === 0) return [];

  // Equal lines get equal numbers, so the search compares integers.
  const ids = new Map<string, number>();
  const number = (line: string) => {
    let id = ids.get(line);
    if (id === undefined) ids.set(line, (id = ids.size));
    return id;
  };
  const a = new Int32Array(n);
  const b = new Int32Array(m);
  for (let i = 0; i < n; i += 1) a[i] = number(base[prefix + i]!);
  for (let j = 0; j < m; j += 1) b[j] = number(current[prefix + j]!);

  const changedA = new Uint8Array(n);
  const changedB = new Uint8Array(m);
  compare(a, b, changedA, changedB);
  return collect(changedA, changedB, prefix);
}

function compare(
  a: Int32Array,
  b: Int32Array,
  changedA: Uint8Array,
  changedB: Uint8Array,
) {
  const width = a.length + b.length + 3;
  const forward = new Int32Array(width);
  const backward = new Int32Array(width);
  // Diagonals run from -b.length - 1 to a.length + 1.
  const offset = b.length + 1;
  const stack: number[] = [0, a.length, 0, b.length];
  while (stack.length > 0) {
    let ylim = stack.pop()!;
    let yoff = stack.pop()!;
    let xlim = stack.pop()!;
    let xoff = stack.pop()!;
    while (xoff < xlim && yoff < ylim && a[xoff] === b[yoff]) {
      xoff += 1;
      yoff += 1;
    }
    while (xlim > xoff && ylim > yoff && a[xlim - 1] === b[ylim - 1]) {
      xlim -= 1;
      ylim -= 1;
    }
    if (xoff === xlim) {
      changedB.fill(1, yoff, ylim);
    } else if (yoff === ylim) {
      changedA.fill(1, xoff, xlim);
    } else {
      const [xmid, ymid] = middleSnake(
        a,
        b,
        forward,
        backward,
        offset,
        xoff,
        xlim,
        yoff,
        ylim,
      );
      stack.push(xmid, xlim, ymid, ylim, xoff, xmid, yoff, ymid);
    }
  }
}

/** A point on a shortest edit path through the box, splitting it in two. */
function middleSnake(
  a: Int32Array,
  b: Int32Array,
  fd: Int32Array,
  bd: Int32Array,
  offset: number,
  xoff: number,
  xlim: number,
  yoff: number,
  ylim: number,
): [number, number] {
  const dmin = xoff - ylim;
  const dmax = xlim - yoff;
  const fmid = xoff - yoff;
  const bmid = xlim - ylim;
  let fmin = fmid;
  let fmax = fmid;
  let bmin = bmid;
  let bmax = bmid;
  const odd = ((fmid - bmid) & 1) !== 0;
  fd[offset + fmid] = xoff;
  bd[offset + bmid] = xlim;
  for (let cost = 1; ; cost += 1) {
    if (fmin > dmin) fd[offset + --fmin - 1] = -1;
    else fmin += 1;
    if (fmax < dmax) fd[offset + ++fmax + 1] = -1;
    else fmax -= 1;
    for (let d = fmax; d >= fmin; d -= 2) {
      const low = fd[offset + d - 1]!;
      const high = fd[offset + d + 1]!;
      let x = low >= high ? low + 1 : high;
      let y = x - d;
      while (x < xlim && y < ylim && a[x] === b[y]) {
        x += 1;
        y += 1;
      }
      fd[offset + d] = x;
      if (odd && bmin <= d && d <= bmax && bd[offset + d]! <= x) return [x, y];
    }
    if (bmin > dmin) bd[offset + --bmin - 1] = 0x7fffffff;
    else bmin += 1;
    if (bmax < dmax) bd[offset + ++bmax + 1] = 0x7fffffff;
    else bmax -= 1;
    for (let d = bmax; d >= bmin; d -= 2) {
      const low = bd[offset + d - 1]!;
      const high = bd[offset + d + 1]!;
      let x = low < high ? low : high - 1;
      let y = x - d;
      while (x > xoff && y > yoff && a[x - 1] === b[y - 1]) {
        x -= 1;
        y -= 1;
      }
      bd[offset + d] = x;
      if (!odd && fmin <= d && d <= fmax && x <= fd[offset + d]!) return [x, y];
    }
    if (cost >= MAX_SNAKE_COST) {
      return [(xoff + xlim) >> 1, (yoff + ylim) >> 1];
    }
  }
}

function collect(
  changedA: Uint8Array,
  changedB: Uint8Array,
  prefix: number,
): LineChange[] {
  const changes: LineChange[] = [];
  let i = 0;
  let j = 0;
  while (i < changedA.length || j < changedB.length) {
    if (
      i < changedA.length &&
      j < changedB.length &&
      !changedA[i] &&
      !changedB[j]
    ) {
      i += 1;
      j += 1;
      continue;
    }
    const baseStart = i;
    const start = j;
    while (i < changedA.length && changedA[i]) i += 1;
    while (j < changedB.length && changedB[j]) j += 1;
    changes.push({
      start: prefix + start,
      end: prefix + j,
      baseStart: prefix + baseStart,
      baseEnd: prefix + i,
    });
  }
  return changes;
}

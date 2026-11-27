import { readFile } from "node:fs/promises";
import path from "node:path";

/** Blanks comments so they cannot hold a rule, keeping newlines. */
function stripComments(source) {
  return source.replace(/\/\*[\s\S]*?(?:\*\/|$)/gu, (match) =>
    match.replace(/[^\n]/gu, " "),
  );
}

const ABSOLUTE = /(?:^|[;{\s])position\s*:\s*absolute\b/iu;
// An inline-axis offset of a whole containing block (`100%`, `-100%`,
// `calc(100% + …)`) puts the box beside its parent, outside the parent's box.
const BESIDE_PARENT =
  /(?:^|[;{\s])(left|right|inset-inline-start|inset-inline-end)\s*:\s*(?:calc\(\s*)?-?100%/iu;

/**
 * Flags a rule that positions a box absolutely beside its parent on the
 * inline axis. An inline control placed like that (a clear button after a
 * chip's name) sits outside the box that paints its hover fill, hit-tests and
 * clips it, so it gets painted-over fills and clipped edges. Make it a flex
 * item of its parent instead.
 */
export function controlsBesideParent(files) {
  const findings = [];
  for (const { relativePath, source } of files) {
    const css = stripComments(source);
    for (const block of css.matchAll(/\{([^{}]*)\}/gu)) {
      const body = block[1];
      if (!ABSOLUTE.test(body)) continue;
      const offset = BESIDE_PARENT.exec(body);
      if (!offset) continue;
      const at = block.index + 1 + offset.index;
      findings.push({
        line: css.slice(0, at).split("\n").length + (offset[0][0] === "\n"),
        property: offset[1].toLowerCase(),
        relativePath,
      });
    }
  }
  return findings;
}

export async function cssInlineControlPolicyErrors(root, cssFiles) {
  const files = await Promise.all(
    cssFiles.map(async (relativePath) => ({
      relativePath: relativePath.split(path.sep).join("/"),
      source: await readFile(path.join(root, relativePath), "utf8"),
    })),
  );
  return controlsBesideParent(files).map(
    ({ line, property, relativePath }) =>
      `${relativePath}:${line} positions an absolute box beside its parent (${property}: 100%); make the control a flex item inside its parent instead.`,
  );
}

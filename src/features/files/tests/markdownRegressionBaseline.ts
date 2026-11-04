import { posix } from "node:path";
import { compileFunction } from "node:vm";
import { ModuleKind, ScriptTarget, transpileModule } from "typescript";
import { createHash } from "node:crypto";
import baseline from "../fixtures/markdown-main-parser.json";
import inputs from "../fixtures/markdown-regression-inputs.json";
import tasks from "../fixtures/gfm-0.29-task-lists.json";
import type { MarkdownPreviewDocument } from "../markdownPreview";
import { visualMarkdownInputs } from "./markdownVisualCorpus";

// Frozen, unmodified sources from origin/main at baseline.commit. Keeping the
// dependency closure makes CI independent of git history and future main edits.
const modules = new Map<string, Record<string, unknown>>();
function load(id: string): Record<string, unknown> {
  const cached = modules.get(id);
  if (cached) return cached;
  const source = baseline.files[id as keyof typeof baseline.files];
  if (source === undefined)
    throw new Error(`Unvendored baseline module: ${id}`);
  const exports = {};
  modules.set(id, exports);
  const { outputText } = transpileModule(source, {
    compilerOptions: {
      module: ModuleKind.CommonJS,
      target: ScriptTarget.ES2022,
    },
  });
  compileFunction(outputText, ["exports", "require"], { filename: id })(
    exports,
    (specifier: string) =>
      load(posix.normalize(posix.join(posix.dirname(id), `${specifier}.ts`))),
  );
  return exports;
}

export const renderMainMarkdown = load("src/features/files/markdownPreview.ts")
  .renderMarkdownPreview as (source: string) => MarkdownPreviewDocument;

// Include every CommonMark example, even those outside the renderer's supported
// conformance subset. Deduplicate exact source strings across all three sources.
export const markdownCorpus = new Map<string, { id: string; name: string }>();
for (const example of inputs.commonmark.examples)
  markdownCorpus.set(example.markdown, {
    id: `commonmark-${example.example}`,
    name: `CommonMark ${example.example}: ${example.section}`,
  });
for (const example of tasks.examples)
  markdownCorpus.set(example.markdown, {
    id: `gfm-${example.example}`,
    name: `GFM ${example.example}: ${example.section}`,
  });
for (const input of [...inputs.testInputs, ...visualMarkdownInputs()])
  if (!markdownCorpus.has(input.markdown))
    markdownCorpus.set(input.markdown, {
      id: `test-${createHash("sha256").update(input.markdown).digest("hex").slice(0, 12)}`,
      name: input.name,
    });

export { baseline };

export const specExamples = new Set([
  ...inputs.commonmark.examples.map(
    ({ example }) => `CommonMark 0.31.2 example ${example}`,
  ),
  ...tasks.examples.map(({ example }) => `GFM 0.29 example ${example}`),
]);

export function documentDigest(document: unknown): string {
  const serialized = JSON.stringify(document, (_key, value) =>
    value && typeof value === "object" && !Array.isArray(value)
      ? Object.fromEntries(
          Object.keys(value)
            .sort()
            .map((key) => [key, value[key]]),
        )
      : value,
  );
  return createHash("sha256").update(serialized).digest("hex");
}

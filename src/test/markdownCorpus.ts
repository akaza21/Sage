import { appendFileSync } from "node:fs";
import { expect, vi } from "vitest";
import type * as MarkdownPreview from "../features/files/markdownPreview";

// Observe actual parser inputs, including generated boundary/streaming cases and
// components rendered indirectly by chat/workbench tests. Only tests load this.
vi.mock("../features/files/markdownPreview", async (importOriginal) => {
  const original = await importOriginal<typeof MarkdownPreview>();
  const { default: inputs } =
    await import("../features/files/fixtures/markdown-regression-inputs.json");
  const { default: tasks } =
    await import("../features/files/fixtures/gfm-0.29-task-lists.json");
  const corpus = new Set([
    ...inputs.testInputs.map(({ markdown }) => markdown),
    ...inputs.commonmark.examples.map(({ markdown }) => markdown),
    ...tasks.examples.map(({ markdown }) => markdown),
  ]);
  return {
    ...original,
    renderMarkdownPreview(source: string) {
      const state = expect.getState();
      if (!state.testPath?.endsWith("markdownRegression.test.ts")) {
        if (process.env.SAGE_MARKDOWN_CORPUS_RECORD === "1")
          appendFileSync(
            "work/markdown-corpus-inputs.jsonl",
            `${JSON.stringify({ name: state.currentTestName, markdown: source })}\n`,
          );
        else
          expect(
            corpus.has(source),
            "Markdown input missing from regression corpus; run node scripts/markdown-corpus.mjs",
          ).toBe(true);
      }
      return original.renderMarkdownPreview(source);
    },
  };
});

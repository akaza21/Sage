import { describe, expect, it } from "vitest";
import { renderMarkdownPreview } from "./markdownPreview";
import {
  markdownCorpus,
  documentDigest,
  renderMainMarkdown,
  specExamples,
} from "./tests/markdownRegressionBaseline";
import intendedDifferences from "./fixtures/markdown-intended-differences.json";

// Compare the entire semantic document: container structure, labels, checkboxes,
// inline formats, links and truncation. A structural match must not hide text loss.

describe("markdown has no unlisted regression against pinned origin/main", () => {
  for (const [markdown, { id, name }] of markdownCorpus) {
    it(`${id}: ${name}`, () => {
      const before = renderMainMarkdown(markdown);
      const after = renderMarkdownPreview(markdown);
      const intended = intendedDifferences.find((entry) => entry.id === id);
      if (documentDigest(before) === documentDigest(after)) {
        expect(intended, "remove stale intended differences").toBeUndefined();
        return;
      }
      expect(
        intended,
        `unlisted difference for ${id}: ${JSON.stringify(markdown.slice(0, 256))}`,
      ).toBeDefined();
      expect(documentDigest(before)).toBe(intended!.before);
      expect(documentDigest(after)).toBe(intended!.after);
    });
  }

  it("accounts for every intended difference with a spec example and reason", () => {
    const ids = new Set([...markdownCorpus.values()].map(({ id }) => id));
    expect(new Set(intendedDifferences.map(({ id }) => id)).size).toBe(
      intendedDifferences.length,
    );
    for (const entry of intendedDifferences) {
      expect(ids.has(entry.id)).toBe(true);
      expect(specExamples.has(entry.spec)).toBe(true);
      expect(entry.reason.length).toBeGreaterThan(0);
    }
  });
});

import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const styles = readFileSync(
  "src/features/chat/acp/acp-chat-activities.css",
  "utf8",
);
const promptStyles = readFileSync(
  "src/features/chat/acp/permission-prompt.css",
  "utf8",
);
const ruleIn = (source: string, selector: string) => {
  const start = source.indexOf(`${selector} {`);
  expect(start, selector).toBeGreaterThanOrEqual(0);
  return source.slice(start, source.indexOf("}", start));
};
const rule = (selector: string) => ruleIn(styles, selector);
const promptRule = (selector: string) => ruleIn(promptStyles, selector);

describe("Chat rich content layout", () => {
  it("keeps body-sized Mermaid labels in a neutral figure with filled subtly bordered nodes", () => {
    const figure = rule(".acp-mermaid");
    expect(figure).toContain("max-width: 100%");
    expect(figure).toContain("min-width: 0");
    expect(figure).not.toContain("overflow: auto");
    expect(figure).not.toMatch(/(?:padding|border|border-radius|background):/u);
    expect(rule(".acp-mermaid svg")).toContain("width: 100%");
    expect(rule(".acp-mermaid svg")).toContain(
      "max-width: var(--mermaid-width)",
    );
    expect(rule(".acp-mermaid svg")).toContain("height: auto");
    expect(rule(".acp-mermaid svg")).toContain(
      "min-width: calc(var(--mermaid-width) * 0.75)",
    );
    expect(rule(".acp-mermaid text")).toContain(
      "font-size: var(--font-size-md)",
    );
    expect(rule(".acp-mermaid :is(rect, polygon)")).toContain(
      "fill: var(--color-panel)",
    );
    expect(rule(".acp-mermaid :is(rect, polygon)")).toContain(
      "stroke: var(--color-border-strong)",
    );
    expect(rule(".acp-mermaid__copy")).toContain(
      "color: var(--color-text-secondary)",
    );
  });
  it("wraps whole approval buttons instead of breaking the words inside them", () => {
    expect(promptRule(".acp-permission-prompt__actions")).toContain(
      "flex-wrap: wrap",
    );
    expect(promptRule(".acp-permission-prompt__actions button")).toContain(
      "flex: none",
    );
    expect(promptRule(".acp-permission-prompt__actions button")).toContain(
      "white-space: nowrap",
    );
  });
});

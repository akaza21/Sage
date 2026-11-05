import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const styles = readFileSync(
  "src/features/chat/acp/acp-chat-activities.css",
  "utf8",
);
const rule = (selector: string) => {
  const start = styles.indexOf(`${selector} {`);
  return styles.slice(start, styles.indexOf("}", start));
};

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
    expect(rule(".acp-mermaid text")).toContain("font-size: inherit");
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
});

import { describe, expect, it } from "vitest";
import { parseMermaidFlowchart } from "./mermaidDiagram";
import { layoutFlowchart } from "./mermaidFlowchartLayout";

describe("Mermaid flowchart centre lines", () => {
  it.each([
    ["TD", "x"],
    ["LR", "y"],
  ] as const)(
    "keeps a mixed-size single-child %s chain on one centre %s",
    (direction, axis) => {
      const diagram = parseMermaidFlowchart(
        `flowchart ${direction}\nOpen[Open link and enter new password] --> Valid{Token and password valid?}\nValid -->|Yes| Done[Hash new password and consume token atomically]`,
      );
      expect(diagram.ok).toBe(true);
      if (!diagram.ok) throw new Error("Expected admitted chain");
      const nodes = layoutFlowchart(diagram);
      expect(nodes).toHaveLength(3);
      expect(
        new Set(nodes.map((node) => (axis === "x" ? node.width : node.height)))
          .size,
      ).toBeGreaterThan(1);
      const centres = nodes.map((node) =>
        axis === "x" ? node.x + node.width / 2 : node.y + node.height / 2,
      );
      for (const centre of centres) expect(centre).toBe(centres[0]);
    },
  );
});

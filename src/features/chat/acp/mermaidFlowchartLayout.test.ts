import { describe, expect, it } from "vitest";
import { parseMermaidFlowchart } from "./mermaidDiagram";
import { layoutFlowchart } from "./mermaidFlowchartLayout";

describe("Mermaid flowchart centre lines", () => {
  it.each([
    ["TD", "x"],
    ["BT", "x"],
    ["LR", "y"],
    ["RL", "y"],
  ] as const)(
    "keeps a mixed-size single-child %s chain on one centre %s",
    (direction, axis) => {
      const diagram = parseMermaidFlowchart(
        `flowchart ${direction}\nOpen[Open link and enter new password] --> Valid{Token and password valid?}\nValid -->|Yes| Done[Hash new password and consume token atomically]`,
      );
      expect(diagram.ok).toBe(true);
      if (!diagram.ok) throw new Error("Expected admitted chain");
      const { nodes } = layoutFlowchart(diagram);
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

describe("Mermaid flowchart direction", () => {
  const chain = (direction: string) => {
    const diagram = parseMermaidFlowchart(
      `flowchart ${direction}\nA[Request] --> B[Verify]\nB --> C[Done]`,
    );
    if (!diagram.ok) throw new Error("Expected admitted chain");
    return layoutFlowchart(diagram);
  };
  const centre = (
    node: ReturnType<typeof chain>["nodes"][number],
    axis: "x" | "y",
  ) => node[axis] + (axis === "x" ? node.width : node.height) / 2;

  it.each([
    ["TD", "y", 1],
    ["TB", "y", 1],
    ["BT", "y", -1],
    ["LR", "x", 1],
    ["RL", "x", -1],
  ] as const)(
    "places a %s chain along %s in reading order %i",
    (direction, axis, sign) => {
      const { nodes, edges } = chain(direction);
      const [a, b, c] = ["A", "B", "C"].map((id) =>
        nodes.find((node) => node.id === id)!,
      );
      expect(Math.sign(centre(b, axis) - centre(a, axis))).toBe(sign);
      expect(Math.sign(centre(c, axis) - centre(b, axis))).toBe(sign);
      expect(Math.min(...nodes.map((node) => node[axis]))).toBe(20);
      for (const edge of edges) {
        const start = edge.points[0][axis === "x" ? 0 : 1];
        const end = edge.points.at(-1)![axis === "x" ? 0 : 1];
        expect(Math.sign(end - start)).toBe(sign);
      }
    },
  );
});

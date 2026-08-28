import { describe, expect, it } from "vitest";

import { parseMermaidFlowchart, parseMermaidSource } from "./mermaidDiagram";

describe("parseMermaidFlowchart", () => {
  it("projects a bounded flowchart without HTML authority", () => {
    const diagram = parseMermaidFlowchart(
      [
        "graph TD",
        "A[Start] --> B{Decide}",
        "B -->|yes| C[Go]",
        "B --> D",
      ].join("\n"),
    );

    expect(diagram).toEqual({
      ok: true,
      direction: "TD",
      nodes: [
        { id: "A", label: "Start", shape: "rect" },
        { id: "B", label: "Decide", shape: "diamond" },
        { id: "C", label: "Go", shape: "rect" },
        { id: "D", label: "D", shape: "rect" },
      ],
      edges: [
        { from: "A", to: "B", label: "" },
        { from: "B", to: "C", label: "yes" },
        { from: "B", to: "D", label: "" },
      ],
    });
  });

  it("rejects unsupported or oversized mermaid as fail-closed", () => {
    expect(parseMermaidFlowchart("<script>alert(1)</script>").ok).toBe(false);
    const overflow = [
      "graph LR",
      ...Array.from(
        { length: 40 },
        (_, index) => `N${index} --> N${index + 1}`,
      ),
    ].join("\n");
    expect(parseMermaidFlowchart(overflow)).toEqual({
      ok: false,
      reason: "limit",
    });
  });
});

describe("parseMermaidSource sequence", () => {
  it("projects a bounded sequence diagram without HTML authority", () => {
    expect(
      parseMermaidSource(
        ["sequenceDiagram", "Alice->>Bob: Hello", "Bob-->>Alice: Hi"].join(
          "\n",
        ),
      ),
    ).toEqual({
      ok: true,
      kind: "sequence",
      actors: ["Alice", "Bob"],
      messages: [
        { from: "Alice", to: "Bob", label: "Hello", dashed: false },
        { from: "Bob", to: "Alice", label: "Hi", dashed: true },
      ],
    });
  });

  it("rejects notes and oversized sequences", () => {
    expect(parseMermaidSource("sequenceDiagram\nNote over A: secret").ok).toBe(
      false,
    );
    const overflow = [
      "sequenceDiagram",
      ...Array.from({ length: 20 }, (_, index) => `A${index}->>B${index}: go`),
    ].join("\n");
    expect(parseMermaidSource(overflow)).toEqual({
      ok: false,
      reason: "limit",
    });
  });
});

describe("parseMermaidSource class", () => {
  it("projects a bounded class diagram without HTML authority", () => {
    expect(
      parseMermaidSource(
        [
          "classDiagram",
          "Animal <|-- Duck",
          "Animal : +int age",
          "Animal : +eat()",
          "Duck --> Wing",
        ].join("\n"),
      ),
    ).toEqual({
      ok: true,
      kind: "class",
      types: [
        { id: "Animal", members: ["+int age", "+eat()"] },
        { id: "Duck", members: [] },
        { id: "Wing", members: [] },
      ],
      relations: [
        { from: "Animal", to: "Duck", kind: "inheritance" },
        { from: "Duck", to: "Wing", kind: "association" },
      ],
    });
  });

  it("rejects notes, style, and oversized class diagrams", () => {
    expect(parseMermaidSource("classDiagram\nNote for Animal: secret").ok).toBe(
      false,
    );
    expect(parseMermaidSource("classDiagram\nstyle Animal fill:#f00").ok).toBe(
      false,
    );
    const overflow = [
      "classDiagram",
      ...Array.from({ length: 16 }, (_, index) => `A${index} --> B${index}`),
    ].join("\n");
    expect(parseMermaidSource(overflow)).toEqual({
      ok: false,
      reason: "limit",
    });
  });
});

describe("parseMermaidSource state", () => {
  it("projects a bounded state diagram without HTML authority", () => {
    expect(
      parseMermaidSource(
        [
          "stateDiagram-v2",
          "[*] --> Still",
          "Still --> Moving : start",
          "Moving --> Crash",
          "Crash --> [*]",
        ].join("\n"),
      ),
    ).toEqual({
      ok: true,
      kind: "state",
      states: [
        { id: "[*]", terminal: true },
        { id: "Still", terminal: false },
        { id: "Moving", terminal: false },
        { id: "Crash", terminal: false },
      ],
      transitions: [
        { from: "[*]", to: "Still", label: "" },
        { from: "Still", to: "Moving", label: "start" },
        { from: "Moving", to: "Crash", label: "" },
        { from: "Crash", to: "[*]", label: "" },
      ],
    });
  });

  it("rejects notes, forks, and oversized state diagrams", () => {
    expect(
      parseMermaidSource("stateDiagram-v2\nnote right of Still: secret").ok,
    ).toBe(false);
    expect(parseMermaidSource("stateDiagram-v2\nstate Fork <<fork>>").ok).toBe(
      false,
    );
    const overflow = [
      "stateDiagram-v2",
      ...Array.from({ length: 16 }, (_, index) => `A${index} --> B${index}`),
    ].join("\n");
    expect(parseMermaidSource(overflow)).toEqual({
      ok: false,
      reason: "limit",
    });
  });
});

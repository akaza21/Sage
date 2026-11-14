import { flowchartSource } from "./cp22Answers";

// Shared by browser geometry tests and the parser regression corpus.
export const geometryListItem = "Wrapped list prose ".repeat(10);
export const geometryOrderedStarts = [1, 9, 10, 100];
export const geometryAnswers = {
  short: "Short completed answer.",
  long: "Long wrapped answer with retained content. ".repeat(400),
  copy: "Copy this answer.",
  code: "```python\nprint('retained')\n```",
  lists: [
    "Prose edge.",
    "",
    "| Database | Status |",
    "| --- | ---: |",
    "| SQLite | Ready |",
    "",
    `- ${geometryListItem}`,
    "",
    // These are independent numbering examples. Blank lines alone keep one
    // CommonMark list, so prose separates each example without weakening checks.
    ...geometryOrderedStarts.flatMap((start, index) => [
      ...(index ? ["Prose edge.", ""] : []),
      `${start}. ${geometryListItem}`,
      "",
    ]),
  ].join("\n"),
};
export function orderedGeometryAnswer(start: number): string {
  return `Prose edge.\n\n${start}. ${geometryListItem}`;
}
export const diagramDirections = ["TD", "LR"];
export const diagramLabelLengths = [28, 64];
export const diagramNodeShapes = [
  ["rect", "[", "]"],
  ["round", "(", ")"],
  ["diamond", "{", "}"],
];
export const returningDiagramDirections = [
  ["TD", "A much longer label here ok"],
  ["LR", "W".repeat(64)],
];
export const diagramCases = [
  ["short flowchart", "flowchart TD\nA --> B"],
  ["wide flowchart", "flowchart LR\nBinding --> Pump\nPump --> SQLite"],
  ["ref 55 flowchart", flowchartSource],
  ["long flowchart edge", `graph TD\nA -->|${"W".repeat(64)}| B`],
  [
    "long class member",
    `classDiagram\nExample : +${"W".repeat(63)}\nExample --> Other`,
  ],
  ["long sequence message", `sequenceDiagram\nAlice->>Bob: ${"W".repeat(64)}`],
];
export function returningDiagramSource(
  direction: string,
  label: string,
  sibling: boolean,
): string {
  return `flowchart ${direction}\nA[Go] --> B[${label}]\nB --> C[Done]\n${sibling ? `A --> D[${label}]\n` : ""}C -->|retry| A`;
}
export function siblingDiagramSource(
  direction: string,
  label: string,
  open: string,
  close: string,
): string {
  return `graph ${direction}\nStart --> Left${open}${label}${close}\nStart --> Right${open}${label}${close}\nRight --> End`;
}

import {
  geometryAnswers,
  geometryOrderedStarts,
  orderedGeometryAnswer,
  diagramCases,
  returningDiagramDirections,
  returningDiagramSource,
  diagramDirections,
  diagramLabelLengths,
  diagramNodeShapes,
  siblingDiagramSource,
} from "../../../../tests/ui/fixtures/markdownAnswerInputs";
import { VISUAL_SCENARIOS } from "../../../../tests/ui/fixtures/visualScenarioCatalog";
import { buildVisualScenario } from "../../../../tests/ui/fixtures/visualScenarioShell";
import { cp22MarkdownAnswers } from "../../../../tests/ui/fixtures/cp22Answers";
import { selectionAnswerText } from "../../../../tests/ui/fixtures/transcriptSelectionFixtures";
import {
  chatChromePrompt,
  chatChromeAnswer,
} from "../../../../tests/ui/fixtures/chatChromeFixture";

// Reuse shared UI fixtures without starting a browser. Additional vectors below
// are the exact inline inputs from the named visual tests, deduplicated upstream.
export function visualMarkdownInputs(): { name: string; markdown: string }[] {
  const inputs: { name: string; markdown: string }[] = [];
  const add = (name: string, ...sources: string[]) => {
    for (const markdown of sources) inputs.push({ name, markdown });
  };
  for (const scenario of VISUAL_SCENARIOS)
    for (const page of buildVisualScenario(scenario.name).pages)
      for (const turn of page.turns)
        for (const item of turn.items)
          if (item.kind === "text")
            add(`UI scenario ${scenario.name}`, item.payload.text);

  const mermaid = (source: string) => `\`\`\`mermaid\n${source}\n\`\`\``;
  add("cp22-answers.visual.ts", ...Object.values(cp22MarkdownAnswers));
  add(
    "cp28-geometry.visual.ts",
    ...Object.values(geometryAnswers),
    ...geometryOrderedStarts.map(orderedGeometryAnswer),
  );
  add(
    "cp31-rich-content.visual.ts",
    ...diagramCases.map(([, source]) => mermaid(source)),
  );
  for (const [direction, label] of returningDiagramDirections)
    for (const sibling of [false, true])
      add(
        "cp31-rich-content.visual.ts",
        mermaid(returningDiagramSource(direction, label, sibling)),
      );
  for (const direction of diagramDirections)
    for (const length of diagramLabelLengths)
      for (const [, open, close] of diagramNodeShapes)
        add(
          "cp31-rich-content.visual.ts",
          mermaid(
            siblingDiagramSource(direction, "W".repeat(length), open, close),
          ),
        );
  add(
    "cp28-terminal.visual.ts",
    `Retained answer with a long token: ${"long-content-".repeat(100)}`,
  );
  add("cp17-running.visual.ts", "Progress follows the live tool row.");
  add("cp17-tool-rows.visual.ts", "Now the build.", "Done.");
  add(
    "fixtures/agentTurnActionScenarios.ts / agentTurnContinuityScenarios.ts",
    "Preserve this Beta draft",
    "Run Alpha once",
    "Run Beta independently",
    "Restore this exact prompt",
    "Uncertain delivery prompt",
    "First terminal Turn",
    "First Turn completed authoritatively.",
    "Explicit follow-up",
    "Follow-up completed.",
    "Latest while scrolled away",
    "Background completion",
    "Background result is complete.",
    "Keep this Turn locked",
  );
  add("fixtures/chatChromeFixture.ts", chatChromePrompt, chatChromeAnswer());
  add(
    "fixtures/transcriptSelectionFixtures.ts",
    selectionAnswerText(false),
    selectionAnswerText(true),
    "This is a temporary discussion of the selected phrase.",
  );
  return inputs;
}

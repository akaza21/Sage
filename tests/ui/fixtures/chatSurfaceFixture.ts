import { acpProjectionStoreFor } from "../../../src/app/acpProjectionRegistry";
import { useWorkbenchStore } from "../../../src/app/store";
import type { AcpItem } from "../../../src/domain/acp";
import { installChatChromeFixture } from "./chatChromeFixture";

export function installChatSurfaceFixture(notice = false) {
  installChatChromeFixture("codex", false);
  const state = useWorkbenchStore.getState();
  const title = "Inspect repo files and math.js";
  useWorkbenchStore.setState({
    tabs: state.tabs.map((tab) => ({ ...tab, title })),
    harnesses: state.harnesses.map((harness) => ({
      ...harness,
      boundVersion: "1.2.3",
      characterisation: notice ? "uncharacterised" : "characterised",
    })),
  });
  for (const tab of state.tabs.filter((entry) => entry.kind === "chat")) {
    const projection = acpProjectionStoreFor(tab.id).getState();
    if (!projection.page || !projection.scope) continue;
    const page = structuredClone(projection.page);
    page.title = title;
    const turn = page.turns[0];
    const basis = turn.items[0];
    const end = Date.parse("2026-08-31T09:29:00Z");
    turn.createdAt = end - 113_000;
    turn.terminalAt = end;
    turn.activities = [];
    turn.browserOperations = [];
    const text = (
      role: "user" | "assistant",
      ordinal: number,
      value: string,
    ): AcpItem => ({
      ...basis,
      id: `surface-${role}-${ordinal}` as AcpItem["id"],
      ordinal,
      createdAt: end,
      kind: "text",
      toolState: null,
      payload: { role, text: value },
    });
    turn.items = [
      text(
        "user",
        1,
        "In this repo: list the files, search for the word greet, read math.js and README.md, and view chart.png. Then run `node -e \"console.log('slow done')\"` and wait for it to finish. After that, write a detailed answer: a table summarising every file, a JavaScript code block with unit tests for math.js, and a short bullet list of observations. Do not modify any files.",
      ),
      text(
        "assistant",
        2,
        "I'll inspect the repository files without modifying anything.",
      ),
      {
        ...basis,
        id: "surface-command" as AcpItem["id"],
        ordinal: 3,
        kind: "toolCall",
        toolState: "completed",
        payload: {
          inputSummary: "node -e \"console.log('slow done')\"",
          outputSummary: "slow done",
          presentation: {
            kind: "execute",
            label: "node -e \"console.log('slow done')\"",
            summary: "node -e \"console.log('slow done')\"",
            cwd: null,
            exitCode: 0,
            outputTail: "slow done",
            outputTruncated: false,
            resources: [],
            activity: "ordinary",
          },
        },
      },
      text(
        "assistant",
        4,
        "Inspected all four repository files without modifying anything.\nThe delayed Node command finished successfully with exit code `0` and printed `slow done`.\n\n| File | Contents | Summary |\n| --- | --- | --- |\n| README.md | Project description | Describes the repository. |\n| package.json | Package metadata | No scripts or dependencies are declared. |\n| math.js | Two exported functions | `add(a, b)` and `greet(name)`. |\n| chart.png | 320 × 180 image | Decorative image with no axes or plotted data. |\n\nSearching for `greet` found one match: its exported declaration at `math.js:5`.\nThe README contains only the heading and a one-sentence project description.\n\n- All files were inspected.\n- No files were modified.",
      ),
    ];
    page.projectionRevision = String(
      BigInt(page.projectionRevision) + 1n,
    ) as typeof page.projectionRevision;
    projection.applyAuthoritativePage(projection.scope, page);
  }
}

import { expect, it } from "vitest";
import type { TurnProjection } from "../../domain/acp";
import { page, textItem } from "./acp/acpProjection.fixture";
import {
  dashWidth,
  sampleTurnIndices,
  turnAnswerPreview,
  turnPrompt,
} from "./turnNavigatorModel";

it("caps an evenly sampled rail and always includes first, last and current", () => {
  const sampled = sampleTurnIndices(2000, 1177);
  expect(sampled).toHaveLength(60);
  expect(sampled).toEqual(
    [...new Set(sampled)].sort((left, right) => left - right),
  );
  expect(sampled).toEqual(expect.arrayContaining([0, 1177, 1999]));
  expect(sampleTurnIndices(2000, 1, 20)).toHaveLength(20);
  expect(sampleTurnIndices(2000, 1, 20)).toEqual(
    expect.arrayContaining([0, 1, 1999]),
  );
  expect(sampleTurnIndices(6, 2)).toEqual([0, 1, 2, 3, 4, 5]);
  expect(sampleTurnIndices(0, 0)).toEqual([]);
});

it("uses the measured fisheye widths", () => {
  expect([0, 1, -1, 2, 3, 4, 20].map(dashWidth)).toEqual([
    24, 18, 18, 13, 9, 6, 6,
  ]);
});

it("bounds previews and preserves plain-text bullets", () => {
  const turn = {
    ...page("completed").turns[0]!,
    items: [
      {
        ...textItem(),
        origin: "primary",
        payload: { role: "user", text: "prompt\n".repeat(30) },
      },
      {
        ...textItem(),
        id: "answer",
        origin: "primary",
        payload: {
          role: "assistant",
          text: "- **Done**\n- " + "answer ".repeat(50),
        },
      },
    ],
  } as TurnProjection;
  const prompt = turnPrompt(turn);
  expect(prompt).toHaveLength(120);
  expect(prompt.endsWith("…")).toBe(true);
  expect(prompt).not.toContain("\n");
  const answer = turnAnswerPreview(turn);
  expect(answer).toHaveLength(240);
  expect(answer.startsWith("• Done\n• ")).toBe(true);
  expect(turnPrompt({ ...turn, items: [], ordinal: 7 })).toBe("Turn 7");
});

import { describe, expect, it } from "vitest";
import type { AcpItem, TurnProjection } from "../../domain/acp";
import { page, textItem } from "./acp/acpProjection.fixture";
import { findChatMatches, stepChatMatch } from "./chatFindModel";

function turn(items: AcpItem[]): TurnProjection {
  return { ...page("completed").turns[0]!, items };
}

function text(id: string, role: "user" | "assistant", value: string) {
  return {
    ...textItem(),
    id,
    origin: "primary",
    payload: { role, text: value },
  } as AcpItem;
}

describe("retained chat find", () => {
  it("gives every match a distinct key, even for identical text", () => {
    const current = turn([
      text("answer", "assistant", "needle\n\nneedle\n\n- needle needle"),
    ]);
    const matches = findChatMatches([current], "needle").matches;
    expect(matches.map((match) => [match.segment, match.start])).toEqual([
      [0, 0],
      [1, 0],
      [2, 0],
      [2, 7],
    ]);
    expect(new Set(matches.map((match) => match.key)).size).toBe(4);
  });

  it("maps case-folded offsets back to the rendered text", () => {
    const source = "\u0130stanbul needle";
    const current = turn([text("answer", "assistant", source)]);
    const [match] = findChatMatches([current], "needle").matches;
    expect(source.slice(match!.start, match!.end)).toBe("needle");
    const [folded] = findChatMatches([current], "i\u0307stanbul").matches;
    expect(source.slice(folded!.start, folded!.end)).toBe("\u0130stanbul");
  });

  it("uses the same final answer as the presentation, excluding empty tails and child/tool text", () => {
    const current = turn([
      text("answer", "assistant", "searchable"),
      { ...text("empty", "assistant", "   "), ordinal: 9 } as AcpItem,
      {
        ...text("child", "assistant", "searchable"),
        origin: "harnessActivity",
        ordinal: 10,
      } as AcpItem,
      {
        ...textItem(),
        kind: "toolCall",
        toolState: "completed",
        origin: "primary",
        payload: {
          inputSummary: "searchable",
          presentation: {
            kind: "other",
            label: "searchable",
            rawKind: null,
            summary: "searchable",
          },
        },
      } as AcpItem,
    ]);
    expect(
      findChatMatches([current], "searchable").matches.map(
        (match) => match.itemId,
      ),
    ).toEqual(["answer"]);
  });

  it("searches only what the renderer shows and says when a message is cut", () => {
    const manyBlocks = turn([
      text("answer", "assistant", "before\n\n".repeat(80) + "needle"),
    ]);
    const cut = findChatMatches([manyBlocks], "needle");
    expect(cut.matches).toHaveLength(0);
    expect(cut.previewLimited).toBe(true);
    const manyRows = turn([
      text(
        "answer",
        "assistant",
        "| title |\n| --- |\n" + "| before |\n".repeat(40) + "| needle |",
      ),
    ]);
    expect(findChatMatches([manyRows], "needle").matches).toHaveLength(0);
    const short = findChatMatches(
      [turn([text("answer", "assistant", "needle")])],
      "needle",
    );
    expect(short).toMatchObject({ previewLimited: false, scanLimited: false });
  });

  it("stops at the scan budget instead of scanning without bound", () => {
    const block = "z".repeat(4000);
    const body = Array.from({ length: 60 }, () => block).join("\n\n");
    const turns = Array.from({ length: 20 }, (_, index) => ({
      ...turn([text(`answer-${index}`, "assistant", body)]),
      id: `turn-${index}` as TurnProjection["id"],
    }));
    expect(findChatMatches(turns, "needle").scanLimited).toBe(true);
    expect(findChatMatches(turns.slice(0, 2), "needle").scanLimited).toBe(
      false,
    );
  });

  it("counts literal case-insensitive prompt and final-answer matches, not activity", () => {
    const current = turn([
      text("prompt", "user", "Subagent [x]"),
      text("activity", "assistant", "subagent commentary"),
      {
        ...textItem(),
        kind: "thoughtSummary",
        payload: { summary: "subagent" },
      } as AcpItem,
      text("answer", "assistant", "Both **subagents** finished. [x]"),
    ]);
    expect(
      findChatMatches([current], "SUBAGENT").matches.map(
        (match) => match.itemId,
      ),
    ).toEqual(["prompt", "answer"]);
    expect(findChatMatches([current], "[x]").matches).toHaveLength(2);
    expect(findChatMatches([current], ".*").matches).toHaveLength(0);
    expect(findChatMatches([current], "").matches).toHaveLength(0);
  });

  it("strips markdown without matching link destinations and maps inline spans", () => {
    const current = turn([
      text(
        "answer",
        "assistant",
        "A **bold** [label](https://hidden.test)\n\n- Next result",
      ),
    ]);
    expect(findChatMatches([current], "bold label").matches).toEqual([
      expect.objectContaining({
        itemId: "answer",
        segment: 0,
        start: 2,
        end: 12,
      }),
    ]);
    expect(findChatMatches([current], "hidden").matches).toHaveLength(0);
  });

  it("caps matches at 500 and distinguishes an exact cap from overflow", () => {
    expect(
      findChatMatches(
        [turn([text("answer", "assistant", "hit ".repeat(500))])],
        "hit",
      ).capped,
    ).toBe(false);
    const result = findChatMatches(
      [turn([text("answer", "assistant", "hit ".repeat(501))])],
      "hit",
    );
    expect(result.matches).toHaveLength(500);
    expect(result.capped).toBe(true);
    const truncated = findChatMatches(
      [turn([text("answer", "assistant", "prefix hit ".repeat(1000))])],
      "hit",
    );
    expect(truncated.matches).toHaveLength(372);
    expect(truncated).toMatchObject({ capped: false, previewLimited: true });
  });

  it("searches unmounted retained turns and wraps both directions", () => {
    const turns = Array.from({ length: 100 }, (_, index) => ({
      ...turn([text(`answer-${index}`, "assistant", "hit")]),
      id: `turn-${index}` as TurnProjection["id"],
    }));
    expect(findChatMatches(turns, "hit").matches.at(-1)?.turnId).toBe(
      "turn-99",
    );
    expect(stepChatMatch(0, -1, 6)).toBe(5);
    expect(stepChatMatch(5, 1, 6)).toBe(0);
    expect(stepChatMatch(0, 1, 0)).toBe(0);
  });
});

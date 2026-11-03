import { expect, it } from "vitest";
import { page, textItem } from "./acp/acpProjection.fixture";
import { chatTurnText } from "./chatTurnText";
import { findChatMatches } from "./chatFindModel";

it("omits rules from copy and find segments without shifting visible text", () => {
  const turn = {
    ...page("completed").turns[0],
    items: [
      {
        ...textItem(),
        payload: { role: "assistant" as const, text: "Before\n\n---\n\nAfter" },
      },
    ],
  };
  const messages = chatTurnText(turn);
  expect(messages[0].segments).toEqual([
    { text: "Before", bullet: false },
    { text: "After", bullet: false },
  ]);
  expect(messages[0].segments.map((segment) => segment.text).join("\n")).toBe(
    "Before\nAfter",
  );
  expect(findChatMatches([turn], "---").matches).toEqual([]);
  expect(findChatMatches([turn], "undefined").matches).toEqual([]);
  expect(findChatMatches([turn], "After").matches[0]).toMatchObject({
    segment: 1,
    start: 0,
    end: 5,
  });
});

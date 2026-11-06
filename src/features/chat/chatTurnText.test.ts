import { expect, it } from "vitest";
import { page, textItem } from "./acp/acpProjection.fixture";
import { chatTurnText } from "./chatTurnText";
import { findChatMatches } from "./chatFindModel";

it("keeps nested answer text searchable and copyable in DOM order", () => {
  const turn = {
    ...page("completed").turns[0],
    items: [
      {
        ...textItem(),
        payload: {
          role: "assistant" as const,
          text: "1. **Run** this command\n\n   ```sh\n   npm install\n   ```\n\n   - Check dependencies\n\n   Then continue\n\n2. Start the app\n\n> Note\n>\n> ```sh\n> npm run dev\n> ```",
        },
      },
    ],
  };
  expect(chatTurnText(turn)[0].segments).toEqual([
    { text: "Run this command", bullet: true },
    { text: "npm install", bullet: false },
    { text: "Check dependencies", bullet: true },
    { text: "Then continue", bullet: true },
    { text: "Start the app", bullet: true },
    { text: "Note", bullet: false },
    { text: "npm run dev", bullet: false },
  ]);
  expect(findChatMatches([turn], "npm install").matches[0]).toMatchObject({
    segment: 1,
    start: 0,
    end: 11,
  });
});

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

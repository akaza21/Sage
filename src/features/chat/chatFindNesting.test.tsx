import { cleanup, render } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { ChatMarkdown } from "./acp/ChatMarkdown";
import { page, textItem } from "./acp/acpProjection.fixture";
import { findChatMatches } from "./chatFindModel";
import { chatMatchRanges } from "./chatFindHighlights";

afterEach(cleanup);

it.each([
  "> Note\n\nAfter",
  "1. Run\n\n   ```sh\n   npm install\n   ```\n\n2. After",
  "- Before\n  - Child\n\n  After",
  "- ```sh\n  npm install\n  ```\n  After",
])("highlights each rendered nested answer segment: %j", (source) => {
  const item = {
    ...textItem(),
    payload: { role: "assistant" as const, text: source },
  };
  const turn = { ...page("completed").turns[0], items: [item] };
  const view = render(
    <article className="acp-message" data-item-id={item.id}>
      <ChatMarkdown text={source} />
    </article>,
  );
  for (const query of source.includes("npm install")
    ? ["npm install", "After"]
    : ["After"]) {
    const matches = findChatMatches([turn], query).matches;
    expect(matches).toHaveLength(1);
    expect(
      chatMatchRanges(view.container, matches).get(matches[0].key)?.toString(),
    ).toBe(query);
  }
});

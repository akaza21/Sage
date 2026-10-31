import { expect, it } from "vitest";
import { page, textItem } from "./acpProjection.fixture";
import { latestRetryPrompt } from "./acpTurnState";

it("replays the saved failed request with its image and annotation identities", () => {
  const saved = page("failed");
  saved.turns[0].items = [
    {
      ...textItem(),
      payload: { role: "user", text: "Inspect **this**\nimage" },
    },
  ];
  saved.turns[0].attachedContext = [
    {
      kind: "image",
      artifactId: "00000000-0000-4000-8000-000000000000",
      mediaType: "image/png",
      width: 1,
      height: 1,
      byteLength: 1,
      sha256: "a".repeat(64),
      label: "sample.png",
    },
    {
      kind: "browserElement",
      target: "element",
      ordinal: 1,
      tabId: "browser-1",
      url: "https://example.com/",
      title: "Example",
      selector: "h1",
      role: "heading",
      name: "Example",
      text: "Example",
      box: { x: 0, y: 0, width: 100, height: 20 },
      comment: "Check heading",
    },
  ];
  expect(latestRetryPrompt(saved)).toMatchObject({
    prompt: "Inspect **this**\nimage",
    attachedContext: saved.turns[0].attachedContext,
  });
});

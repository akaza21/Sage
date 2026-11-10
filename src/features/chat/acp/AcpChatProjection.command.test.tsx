import { cleanup, render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { AcpItem, TurnProjection } from "../../../domain/acp";
import { settleLazyModules } from "../../../test/settleLazyModules";
import { AcpChatProjection } from "./AcpChatProjection";
import { page, textItem } from "./acpProjection.fixture";

async function promptBubble(
  text: string,
  marks: Pick<TurnProjection, "command" | "sessionAction">,
) {
  const next = page("completed");
  Object.assign(next.turns[0], marks);
  next.turns[0].items = [
    {
      ...textItem(),
      id: "user-prompt" as AcpItem["id"],
      payload: { role: "user", text },
    },
    textItem(),
  ];
  const { container } = render(<AcpChatProjection page={next} active />);
  await settleLazyModules();
  return container.querySelector<HTMLElement>(".acp-message--user")!;
}

describe("a prompt that ran a listed command", () => {
  it("shows the command and its source, then what was typed after it", async () => {
    const bubble = await promptBubble("/deploy  staging\nnow", {
      command: { name: "deploy", source: "skill" },
    });
    const command = bubble.querySelector(".acp-message__command");
    expect(command?.textContent).toBe("/deploySkill");
    expect(
      command?.querySelector(".acp-message__command-name")?.textContent,
    ).toBe("/deploy");
    const body = bubble.querySelector(".acp-markdown");
    expect(body?.textContent).toContain("staging");
    expect(body?.textContent).not.toContain("/deploy");
  });

  it("shows a command with no input as the chip alone", async () => {
    const bubble = await promptBubble("/cost", {
      command: { name: "cost", source: "builtin" },
    });
    expect(bubble.querySelector(".acp-message__command")?.textContent).toBe(
      "/costBuilt-in",
    );
    expect(bubble.querySelector(".acp-markdown")).toBeNull();
  });

  it("shows a session action as its harness's own command", async () => {
    const bubble = await promptBubble("/compact", {
      sessionAction: { kind: "compact" },
    });
    expect(bubble.querySelector(".acp-message__command")?.textContent).toBe(
      "/compactBuilt-in",
    );
  });

  it("names no source the harness left unclassified", async () => {
    const bubble = await promptBubble("/plan the work", {
      command: { name: "plan", source: "harness" },
    });
    expect(bubble.querySelector(".acp-message__command")?.textContent).toBe(
      "/plan",
    );
    expect(bubble.querySelector(".acp-message__command-source")).toBeNull();
  });

  it("keeps the text as typed when it does not start with the command", async () => {
    for (const text of ["/deployment staging", "deploy staging"]) {
      const bubble = await promptBubble(text, {
        command: { name: "deploy", source: "skill" },
      });
      expect(bubble.querySelector(".acp-message__command")).toBeNull();
      expect(bubble.querySelector(".acp-markdown")?.textContent).toContain(
        text,
      );
      cleanup();
    }
  });

  it("shows a mark that reaches an already rendered prompt", async () => {
    const before = page("completed");
    before.turns[0].items = [
      {
        ...textItem(),
        id: "user-prompt" as AcpItem["id"],
        payload: { role: "user", text: "/deploy staging" },
      },
    ];
    const view = render(<AcpChatProjection page={before} active />);
    await settleLazyModules();
    expect(view.container.querySelector(".acp-message__command")).toBeNull();
    const after = structuredClone(before);
    after.turns[0].command = { name: "deploy", source: "skill" };
    view.rerender(<AcpChatProjection page={after} active />);
    await settleLazyModules();
    expect(
      view.container.querySelector(".acp-message__command")?.textContent,
    ).toBe("/deploySkill");
  });

  it("is plain text without a mark", async () => {
    const bubble = await promptBubble("/deploy staging", {});
    expect(bubble.querySelector(".acp-message__command")).toBeNull();
    expect(bubble.querySelector(".acp-markdown")?.textContent).toContain(
      "/deploy staging",
    );
  });
});

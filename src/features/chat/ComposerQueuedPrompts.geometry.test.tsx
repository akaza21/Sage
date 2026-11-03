import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { useRef, useState } from "react";
import { ComposerQueuedPrompts } from "./ComposerQueuedPrompts";
import type { QueuedChatPrompt } from "../../domain/chatOwnership";

afterEach(cleanup);

it("removes queued flow rows and returns focus to their own composer", () => {
  function Chat({ name }: { name: string }) {
    const input = useRef<HTMLTextAreaElement>(null);
    const [prompts, setPrompts] = useState<QueuedChatPrompt[]>([
      {
        id: name,
        text: name,
        attachedContext: [],
        bindingId: "binding",
        modelId: null,
        status: "blocked",
      },
    ]);
    return (
      <form className="composer">
        <ComposerQueuedPrompts
          composerRef={input}
          disabled={false}
          prompts={prompts}
          onDelete={(id) =>
            setPrompts((rows) => rows.filter((row) => row.id !== id))
          }
        />
        <textarea ref={input} aria-label={name} />
      </form>
    );
  }
  render(
    <>
      <Chat name="First chat" />
      <Chat name="Side chat" />
    </>,
  );
  fireEvent.click(
    screen.getByRole("button", { name: "Delete queued message: Side chat" }),
  );
  expect(screen.getByRole("textbox", { name: "Side chat" })).toHaveFocus();
  expect(
    screen.queryByText("Side chat", { selector: ".composer__queued-text" }),
  ).toBeNull();
  expect(
    screen.getByText("First chat", { selector: ".composer__queued-text" }),
  ).toBeVisible();
  fireEvent.click(
    screen.getByRole("button", { name: "Delete queued message: First chat" }),
  );
  expect(screen.getByRole("textbox", { name: "First chat" })).toHaveFocus();
  expect(screen.queryByLabelText("Queued messages")).toBeNull();
});

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

it("keeps a queued message with images off text-only Steer so it waits for the normal send", () => {
  const row = (
    id: string,
    attachedContext: QueuedChatPrompt["attachedContext"],
  ) =>
    ({
      id,
      text: id,
      attachedContext,
      bindingId: "binding",
      modelId: null,
      status: "queued",
    }) satisfies QueuedChatPrompt;
  render(
    <ComposerQueuedPrompts
      disabled={false}
      steerAvailable
      prompts={[
        row("Look at this", [
          {
            kind: "image",
            artifactId: "a77c3f00-487c-40b5-87ac-236de6843bd1",
            mediaType: "image/png",
            width: 1200,
            height: 800,
            byteLength: 1024,
            sha256: "a".repeat(64),
            label: "screenshot.png",
          },
        ]),
        row("Change direction", []),
      ]}
    />,
  );
  expect(
    screen.queryByRole("button", {
      name: "Send now: Look at this",
    }),
  ).toBeNull();
  expect(
    screen.getByRole("button", {
      name: "Send now: Change direction",
    }),
  ).toBeEnabled();
  expect(
    screen.getByRole("region", { name: "Queued messages" }),
  ).toHaveTextContent("Queued messages2Sends after the agent finishes");
});

it("puts a text-only queued message back in the composer and folds the card to its header", () => {
  function Chat() {
    const input = useRef<HTMLTextAreaElement>(null);
    const [text, setText] = useState("Draft");
    const [prompts, setPrompts] = useState<QueuedChatPrompt[]>([
      {
        id: "q1",
        text: "Also check the branch",
        attachedContext: [],
        bindingId: "binding",
        modelId: null,
        status: "queued",
      },
    ]);
    return (
      <>
        <ComposerQueuedPrompts
          composerRef={input}
          disabled={false}
          awaitingApproval
          prompts={prompts}
          onDelete={(id) =>
            setPrompts((rows) => rows.filter((row) => row.id !== id))
          }
          onEdit={(queued) => setText(`${text}
${queued}`)}
        />
        <textarea
          ref={input}
          aria-label="Prompt"
          value={text}
          onChange={(event) => setText(event.target.value)}
        />
      </>
    );
  }
  render(<Chat />);
  const card = screen.getByRole("region", { name: "Queued messages" });
  expect(card).toHaveTextContent("Waiting on your approval");
  const fold = screen.getByRole("button", {
    name: "Collapse queued messages",
  });
  fireEvent.click(fold);
  expect(fold).toHaveAccessibleName("Expand queued messages");
  expect(fold).toHaveAttribute("aria-expanded", "false");
  expect(screen.queryByText("Also check the branch")).toBeNull();
  fireEvent.click(fold);
  fireEvent.click(
    screen.getByRole("button", {
      name: "Edit queued message: Also check the branch",
    }),
  );
  const prompt = screen.getByRole("textbox", { name: "Prompt" });
  expect(prompt).toHaveValue("Draft
Also check the branch");
  expect(prompt).toHaveFocus();
  expect(screen.queryByRole("region", { name: "Queued messages" })).toBeNull();
});

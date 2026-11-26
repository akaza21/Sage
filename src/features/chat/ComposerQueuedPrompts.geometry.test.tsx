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

it("offers Steer only for queued messages it can carry, so images wait for their turn", () => {
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
      name: "Steer queued message: Look at this",
    }),
  ).toBeNull();
  expect(
    screen.getByRole("button", {
      name: "Steer queued message: Change direction",
    }),
  ).toBeEnabled();
  expect(screen.queryByText("Needs retry")).toBeNull();
  expect(screen.getAllByText("Queued")).toHaveLength(2);
});

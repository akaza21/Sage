import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { useState } from "react";
import { afterEach, expect, it } from "vitest";
import { ComposerChrome } from "./ComposerChrome";

afterEach(cleanup);

const surface = (key: string, text: string) => ({
  activities: [],
  chrome: { editorText: { key, text } },
});

/**
 * A composer as the chat region drives it: sending clears the field, and a
 * failed send puts the prompt back only into an empty field.
 */
function Composer({
  scope,
  offer,
  draft,
  sendFails = false,
}: {
  scope: string;
  offer: unknown;
  draft: string;
  sendFails?: boolean;
}) {
  const [text, setText] = useState(draft);
  return (
    <form
      onSubmit={(event) => {
        event.preventDefault();
        const prompt = text;
        setText("");
        if (sendFails)
          queueMicrotask(() => setText((current) => current || prompt));
      }}
    >
      <textarea
        aria-label="Message agent"
        value={text}
        onChange={(event) => setText(event.target.value)}
      />
      <ComposerChrome
        chatAuth={{ page: { sessionSurface: offer } as never, errorKind: null }}
        dictationScope={scope}
        text={text}
        onTextChange={setText}
      />
      <button type="submit">Send</button>
    </form>
  );
}

const field = () => screen.getByLabelText<HTMLTextAreaElement>("Message agent");

it("never overwrites a draft with harness-suggested text", () => {
  const { rerender } = render(
    <Composer scope="a" offer={surface("7-1", "Suggested")} draft="My draft" />,
  );
  expect(field().value).toBe("My draft");
  fireEvent.click(screen.getByRole("button", { name: "Ignore suggestion" }));
  expect(field().value).toBe("My draft");
  expect(screen.queryByText(/The agent suggests text/)).toBeNull();
  // The ignored offer stays answered once the draft is cleared.
  fireEvent.change(field(), { target: { value: "" } });
  rerender(<Composer scope="a" offer={surface("7-1", "Suggested")} draft="" />);
  expect(field().value).toBe("");
  // A new offer fills only the now empty composer.
  rerender(<Composer scope="a" offer={surface("7-2", "Second")} draft="" />);
  expect(field().value).toBe("Second");
  expect(screen.getByRole("status").textContent).toBe(
    "Suggested text added to the message.",
  );
});

it("keeps the composer empty after sending past an offer shown beside the draft", () => {
  render(
    <Composer scope="b" offer={surface("7-1", "Suggested")} draft="My draft" />,
  );
  expect(screen.getByText(/The agent suggests text/)).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Send" }));
  expect(field().value).toBe("");
  expect(screen.queryByText(/The agent suggests text/)).toBeNull();
});

it("restores the user's prompt, not the offer, when a send fails", async () => {
  render(
    <Composer
      scope="c"
      offer={surface("7-1", "Suggested")}
      draft="My draft"
      sendFails
    />,
  );
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
  });
  expect(field().value).toBe("My draft");
});

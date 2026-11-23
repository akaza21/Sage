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
 * A composer as the chat region drives it: a send clears the field, a failed
 * send puts the prompt back only into an empty field, and a blocked submit
 * changes nothing.
 */
function Composer({
  scope,
  offer,
  draft,
  send = "sent",
}: {
  scope: string;
  offer: unknown;
  draft: string;
  send?: "sent" | "fails" | "blocked";
}) {
  const [text, setText] = useState(draft);
  return (
    <form
      onSubmit={(event) => {
        event.preventDefault();
        if (send === "blocked") return;
        const prompt = text;
        setText("");
        if (send === "fails")
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
const banner = () =>
  screen.queryByText(/The agent suggests text/, { selector: ".notice span" });

it("never changes the composer by itself, even when a send fails mid-offer", async () => {
  const { rerender } = render(
    <Composer
      scope="a"
      offer={surface("7-1", "Suggested")}
      draft=""
      send="fails"
    />,
  );
  expect(field().value).toBe("");
  expect(banner()).toBeTruthy();
  fireEvent.change(field(), { target: { value: "My prompt" } });
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    // A new offer arrives while the send is in flight.
    rerender(
      <Composer
        scope="a"
        offer={surface("7-2", "Second")}
        draft=""
        send="fails"
      />,
    );
  });
  expect(field().value).toBe("My prompt");
});

it("applies an offer only when the user uses it, and ignoring keeps the draft", () => {
  const { rerender } = render(
    <Composer scope="b" offer={surface("7-1", "Suggested")} draft="" />,
  );
  fireEvent.click(screen.getByRole("button", { name: "Use suggestion" }));
  expect(field().value).toBe("Suggested");
  expect(banner()).toBeNull();
  rerender(<Composer scope="b" offer={surface("7-2", "Second")} draft="" />);
  fireEvent.click(
    screen.getByRole("button", { name: "Replace draft with suggestion" }),
  );
  expect(field().value).toBe("Second");
  rerender(<Composer scope="b" offer={surface("7-3", "Third")} draft="" />);
  fireEvent.click(screen.getByRole("button", { name: "Ignore suggestion" }));
  expect(field().value).toBe("Second");
  expect(banner()).toBeNull();
});

it("keeps the offer when a submit is blocked", () => {
  render(
    <Composer
      scope="c"
      offer={surface("7-1", "Suggested")}
      draft="My draft"
      send="blocked"
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: "Send" }));
  expect(field().value).toBe("My draft");
  expect(banner()).toBeTruthy();
});

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { afterEach, expect, it } from "vitest";
import { ComposerChrome } from "./ComposerChrome";

afterEach(cleanup);

const surface = (key: string, text: string) => ({
  activities: [],
  chrome: { editorText: { key, text } },
});

function Composer({ offer, draft }: { offer: unknown; draft: string }) {
  const [text, setText] = useState(draft);
  return (
    <>
      <textarea
        aria-label="Message agent"
        value={text}
        onChange={(event) => setText(event.target.value)}
      />
      <ComposerChrome
        place="below"
        chatAuth={{ page: { sessionSurface: offer } as never, errorKind: null }}
        dictationScope="chat-1"
        text={text}
        onTextChange={setText}
      />
    </>
  );
}

it("never overwrites a draft with harness-suggested text", () => {
  const { rerender } = render(
    <Composer offer={surface("7-1", "From the extension")} draft="My draft" />,
  );
  const composer = screen.getByLabelText<HTMLTextAreaElement>("Message agent");
  expect(composer.value).toBe("My draft");
  expect(screen.getByText("The agent suggests text")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Ignore" }));
  expect(composer.value).toBe("My draft");
  expect(screen.queryByText("The agent suggests text")).toBeNull();
  // The ignored offer stays handled even once the draft is cleared.
  fireEvent.change(composer, { target: { value: "" } });
  rerender(<Composer offer={surface("7-1", "From the extension")} draft="" />);
  expect(composer.value).toBe("");
  // A new offer fills only the now empty composer.
  rerender(<Composer offer={surface("7-2", "Second offer")} draft="" />);
  expect(composer.value).toBe("Second offer");
});

import { act, fireEvent, render, screen } from "@testing-library/react";
import { useRef, useState } from "react";
import { expect, it, vi } from "vitest";
import { ComposerSlashController } from "./ComposerSlashController";

const chunk = vi.hoisted(() => {
  let fail: (error: Error) => void = () => {};
  const gate = new Promise<never>((_, reject) => {
    fail = reject;
  });
  return { gate, fail: (error: Error) => fail(error) };
});
vi.mock("./ComposerSlashMenu", async () => chunk.gate);

const composerKeys = vi.fn();

function Field() {
  const fieldRef = useRef<HTMLTextAreaElement>(null);
  const readyRef = useRef(false);
  const [text, setText] = useState("");
  return (
    <>
      <textarea
        aria-label="Message"
        ref={fieldRef}
        value={text}
        onChange={(event) => setText(event.target.value)}
        onKeyDown={(event) => composerKeys(event.key, event.defaultPrevented)}
      />
      <ComposerSlashController
        fieldRef={fieldRef}
        boundChatId="chat-a"
        disabled={false}
        fieldId="message"
        onTextChange={setText}
        readyRef={readyRef}
        text={text}
      />
    </>
  );
}

it("names no listbox before the menu mounts and gives way when its chunk fails", async () => {
  render(<Field />);
  const field = screen.getByLabelText<HTMLTextAreaElement>("Message");
  act(() => field.focus());
  fireEvent.change(field, { target: { value: "/comp" } });
  fireEvent.input(field);
  expect(field.getAttribute("aria-expanded")).toBe("false");
  expect(field.hasAttribute("aria-controls")).toBe(false);
  fireEvent.keyDown(field, { key: "Enter" });
  expect(composerKeys).not.toHaveBeenCalled();

  await act(async () => {
    chunk.fail(new Error("offline"));
    await Promise.resolve();
  });
  await act(async () => {});
  expect(field.getAttribute("aria-expanded")).toBe("false");
  expect(field.hasAttribute("aria-controls")).toBe(false);
  fireEvent.keyDown(field, { key: "Enter" });
  expect(composerKeys).toHaveBeenCalledWith("Enter", false);
});

import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { useRef, useState } from "react";
import { afterEach, expect, it, vi } from "vitest";
import { ComposerTurnControl } from "./ComposerTurnControl";
import { useComposerDictation } from "./composerChrome";
import type { BrowserRecognition } from "./composerDictationRuntime";

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

it("changes one control from mic to send to stop and disables dictation during the turn", async () => {
  class Speech implements BrowserRecognition {
    static latest: Speech;
    continuous = false;
    interimResults = false;
    lang = "";
    onresult: BrowserRecognition["onresult"] = null;
    onerror: BrowserRecognition["onerror"] = null;
    onend: BrowserRecognition["onend"] = null;
    constructor() {
      Speech.latest = this;
    }
    start() {}
    stop() {
      this.onend?.();
    }
    abort() {
      this.onend?.();
    }
    emit(text: string) {
      this.onresult?.({
        resultIndex: 0,
        results: [{ isFinal: true, 0: { transcript: text } }],
      });
    }
  }
  vi.stubGlobal("SpeechRecognition", Speech);
  let dictation!: ReturnType<typeof useComposerDictation>;
  function Composer() {
    const input = useRef<HTMLTextAreaElement>(null);
    const [text, setText] = useState("");
    const [activeTurn, setActiveTurn] = useState(false);
    dictation = useComposerDictation(text, setText, "chat", activeTurn, input);
    return (
      <form
        onSubmit={(event) => {
          event.preventDefault();
          setActiveTurn(true);
        }}
      >
        <textarea
          ref={input}
          aria-label="Prompt"
          value={text}
          onChange={(event) => setText(event.target.value)}
        />
        <ComposerTurnControl
          dictation={dictation}
          activeTurn={activeTurn}
          hasContent={Boolean(text.trim())}
          onCancelTurn={() => setActiveTurn(false)}
          cancelDisabled={false}
          sendDisabled={!text.trim()}
        />
      </form>
    );
  }
  render(<Composer />);
  const control = screen.getByRole("button", { name: "Start dictation" });
  expect(control).toBeEnabled();
  fireEvent.click(control);
  await waitFor(() => expect(dictation.listening).toBe(true));
  act(() => Speech.latest.emit("Review the composer"));
  const input = screen.getByRole("textbox", { name: "Prompt" });
  expect(input).toHaveValue("Review the composer");
  expect(screen.getByRole("button", { name: "Send message" })).toBe(control);
  fireEvent.click(control);
  expect(screen.getByRole("button", { name: "Cancel turn" })).toBe(control);
  expect(dictation.disabled).toBe(true);
  expect(screen.getAllByRole("button")).toHaveLength(1);
  expect(screen.queryByRole("button", { name: "Start dictation" })).toBeNull();
  expect(control).not.toHaveAttribute("aria-pressed");
  expect(control).toHaveAttribute(
    "title",
    "Voice input unavailable during a turn",
  );
  await act(async () => {
    await dictation.start();
  });
  act(() => Speech.latest.emit("late speech"));
  expect(input).toHaveValue("Review the composer");
  fireEvent.click(control);
  expect(screen.getByRole("button", { name: "Send message" })).toBe(control);
  fireEvent.change(input, { target: { value: "" } });
  expect(screen.getByRole("button", { name: "Start dictation" })).toBe(control);
  expect(control).toBeEnabled();
});

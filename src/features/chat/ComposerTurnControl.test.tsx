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
import type * as NativeInvoke from "../../ports/nativeInvoke";

const native = vi.hoisted(() => ({
  enabled: false,
  id: "",
  emit: null as null | ((payload: unknown) => void),
}));
vi.mock("../../ports/nativeInvoke", async (importOriginal) => {
  const actual = await importOriginal<typeof NativeInvoke>();
  return {
    ...actual,
    isTauriRuntime: () => native.enabled,
    invokeNative: async (_command: string, args: { id: string }) => {
      native.id = args.id;
    },
  };
});
vi.mock("@tauri-apps/api/event", () => ({
  listen: async (
    _name: string,
    callback: (event: { payload: unknown }) => void,
  ) => {
    native.emit = (payload) => callback({ payload });
    return () => {
      native.emit = null;
    };
  },
}));

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  native.enabled = false;
  native.emit = null;
});

it("keeps dictation on one control until stopped and disables it during the turn", async () => {
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
  expect(screen.getByRole("button", { name: "Stop dictation" })).toBe(control);
  expect(control).toHaveAttribute("aria-pressed", "true");
  fireEvent.click(control);
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

it("keeps a native partial speech draft when stopped and sends it only on Send", async () => {
  native.enabled = true;
  vi.spyOn(navigator, "platform", "get").mockReturnValue("Win32");
  function Composer() {
    const input = useRef<HTMLTextAreaElement>(null);
    const [text, setText] = useState("");
    const [sent, setSent] = useState("");
    const dictation = useComposerDictation(text, setText, "chat", false, input);
    return (
      <form
        onSubmit={(event) => {
          event.preventDefault();
          setSent(text);
          setText("");
        }}
      >
        <textarea ref={input} aria-label="Prompt" value={text} readOnly />
        <output aria-label="Sent prompt">{sent}</output>
        <ComposerTurnControl
          dictation={dictation}
          activeTurn={false}
          hasContent={Boolean(text.trim())}
          onCancelTurn={null}
          cancelDisabled={false}
          sendDisabled={!text.trim()}
        />
      </form>
    );
  }
  render(<Composer />);
  const control = screen.getByRole("button", { name: "Start dictation" });
  fireEvent.click(control);
  await screen.findByRole("button", { name: "Stop dictation" });
  act(() =>
    native.emit?.({
      id: native.id,
      kind: "partial",
      text: "Review the composer",
    }),
  );
  const input = screen.getByRole("textbox", { name: "Prompt" });
  expect(input).toHaveValue("Review the composer");
  expect(screen.getByRole("button", { name: "Stop dictation" })).toBe(control);
  expect(control).toHaveClass("composer__mic");
  expect(control).toHaveAttribute("aria-pressed", "true");
  fireEvent.click(control);
  expect(input).toHaveValue("Review the composer");
  expect(
    screen.getByRole("status", { name: "Sent prompt" }),
  ).toBeEmptyDOMElement();
  expect(screen.getByRole("button", { name: "Send message" })).toBe(control);
  expect(control).not.toHaveAttribute("aria-pressed");
  fireEvent.click(control);
  expect(screen.getByRole("status", { name: "Sent prompt" })).toHaveTextContent(
    "Review the composer",
  );
  expect(input).toHaveValue("");
});

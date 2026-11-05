import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { createRef } from "react";
import { useComposerDictation } from "./composerChrome";
vi.mock("../../ports/nativeInvoke", () => ({
  isTauriRuntime: () => false,
  invokeNative: vi.fn(),
}));
class Speech {
  static latest: Speech;
  onresult:
    | ((event: {
        resultIndex: number;
        results: { isFinal: boolean; 0: { transcript: string } }[];
      }) => void)
    | null = null;
  onerror: (() => void) | null = null;
  onend: (() => void) | null = null;
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
beforeEach(() => vi.stubGlobal("SpeechRecognition", Speech));
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});
it("appends final speech to the current draft and ignores late speech after cancellation", async () => {
  const input = document.createElement("textarea");
  const ref = createRef<HTMLTextAreaElement>();
  Object.defineProperty(ref, "current", { value: input });
  input.value = "original";
  const { result } = renderHook(() =>
    useComposerDictation(
      "original",
      (text) => {
        input.value = text;
      },
      "chat",
      false,
      ref,
    ),
  );
  await act(async () => result.current.start());
  input.value = "edited draft";
  act(() => Speech.latest.emit("hello world"));
  expect(input.value).toBe("edited draft hello world");
  expect(result.current.listening).toBe(true);
  act(() => window.dispatchEvent(new Event("blur")));
  expect(result.current.listening).toBe(false);
  act(() => Speech.latest.emit("late"));
  expect(input.value).toBe("edited draft hello world");
});
it("reports microphone failure without changing the draft", async () => {
  const input = document.createElement("textarea");
  const ref = createRef<HTMLTextAreaElement>();
  Object.defineProperty(ref, "current", { value: input });
  input.value = "draft";
  const { result } = renderHook(() =>
    useComposerDictation(
      "draft",
      (text) => {
        input.value = text;
      },
      "chat",
      false,
      ref,
    ),
  );
  await act(async () => result.current.start());
  act(() => Speech.latest.onerror?.());
  expect(result.current.error).toMatch(/microphone access/);
  expect(result.current.listening).toBe(false);
  expect(input.value).toBe("draft");
});

import {
  useEffect,
  useCallback,
  useRef,
  useState,
  type RefObject,
} from "react";

import {
  keymapBinding,
  matchesKeymapChord,
  type KeymapChordEvent,
} from "../../domain/keymapRegistry";
import type { AcpConfigOption } from "../../domain/acpSessionConfig";
import { flattenChoices } from "../../domain/acpSessionConfigControls";
import { MAX_CHAT_COMPOSER_BYTES } from "../../domain/chatOwnership";
import { utf8ByteLength } from "../../domain/text";
import { invokeNative, isTauriRuntime } from "../../ports/nativeInvoke";
import { shortcutLabel } from "../../ui/shortcutLabel";

export { shortcutLabel, usesMetaModifier } from "../../ui/shortcutLabel";

export const COMPOSER_PROMPT_PLACEHOLDER = "Do anything";
export const COMPOSER_PROMPT_PLACEHOLDER_DISABLED =
  "Choose an available harness before starting a session";
export const COMPOSER_WORKSPACE_NAME_CAP = 48;

interface VoiceEvent {
  id: string;
  kind: "partial" | "final" | "ended" | "error";
  text?: string;
  message?: string;
}

interface BrowserRecognition {
  continuous: boolean;
  interimResults: boolean;
  lang: string;
  onresult:
    | ((event: {
        resultIndex: number;
        results: ArrayLike<{
          isFinal: boolean;
          [index: number]: { transcript: string };
        }>;
      }) => void)
    | null;
  onerror: (() => void) | null;
  onend: (() => void) | null;
  start: () => void;
  stop: () => void;
  abort: () => void;
}

function browserRecognition() {
  const speech = window as Window & {
    SpeechRecognition?: new () => BrowserRecognition;
    webkitSpeechRecognition?: new () => BrowserRecognition;
  };
  return speech.SpeechRecognition ?? speech.webkitSpeechRecognition;
}

interface ActiveDictation {
  id: string;
  base: string;
  start: number;
  end: number;
  unlisten?: () => void;
}

export function useComposerDictation(
  text: string,
  onTextChange: (value: string) => void,
  scope: string,
  suspended: boolean,
  inputRef: RefObject<HTMLTextAreaElement | null>,
) {
  const [listeningState, setListening] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const active = useRef<ActiveDictation | null>(null);
  const browser = useRef<BrowserRecognition | null>(null);
  const generation = useRef(0);
  const change = useRef(onTextChange);
  useEffect(() => {
    change.current = onTextChange;
  }, [onTextChange]);
  const cancel = useCallback(() => {
    browser.current?.abort();
    browser.current = null;
    const current = active.current;
    active.current = null;
    current?.unlisten?.();
    if (current) {
      void invokeNative("stop_voice_input", {
        id: current.id,
        cancel: true,
      }).catch(() => undefined);
    }
    setListening(false);
  }, []);
  useEffect(() => {
    if (suspended) queueMicrotask(cancel);
  }, [cancel, suspended]);
  useEffect(() => () => cancel(), [cancel, scope]);
  useEffect(() => {
    window.addEventListener("blur", cancel);
    return () => window.removeEventListener("blur", cancel);
  }, [cancel]);

  const nativeSupported = isTauriRuntime() && /win/i.test(navigator.platform);
  const supported = nativeSupported || Boolean(browserRecognition());
  const start = async () => {
    if (!supported || suspended) return;
    if (!nativeSupported) {
      const Constructor = browserRecognition();
      if (!Constructor) return;
      cancel();
      try {
        const next = new Constructor();
        next.continuous = false;
        next.interimResults = false;
        next.lang = navigator.language;
        next.onresult = (event) => {
          if (browser.current !== next) return;
          const spoken = Array.from(event.results)
            .slice(event.resultIndex)
            .filter((result) => result.isFinal)
            .map((result) => result[0]?.transcript ?? "")
            .join(" ")
            .trim();
          if (!spoken) return;
          const base = inputRef.current?.value ?? text;
          const updated = `${base}${base && !/\s$/.test(base) ? " " : ""}${spoken}`;
          if (utf8ByteLength(updated) > MAX_CHAT_COMPOSER_BYTES) {
            setError("Dictation is too long for this message.");
            cancel();
            return;
          }
          change.current(updated);
        };
        next.onerror = () => {
          if (browser.current === next) {
            setError(
              "Dictation failed. Check microphone access and try again.",
            );
            cancel();
          }
        };
        next.onend = () => {
          if (browser.current === next) {
            browser.current = null;
            setListening(false);
          }
        };
        browser.current = next;
        setError(null);
        next.start();
        setListening(true);
      } catch {
        cancel();
        setError("Dictation could not start. Check microphone access.");
      }
      return;
    }
    const input = inputRef.current;
    const id =
      globalThis.crypto?.randomUUID?.() ??
      `${Date.now()}-${++generation.current}`;
    const current: ActiveDictation = {
      id,
      base: text,
      start: input?.selectionStart ?? text.length,
      end: input?.selectionEnd ?? text.length,
    };
    cancel();
    active.current = current;
    setError(null);
    try {
      const { listen } = await import("@tauri-apps/api/event");
      current.unlisten = await listen<VoiceEvent>("voice-input", (event) => {
        const payload = event.payload;
        if (active.current !== current || payload.id !== id) return;
        if (payload.kind === "error") {
          setError(
            payload.message ?? "Dictation failed. Check microphone access.",
          );
          cancel();
          return;
        }
        if (payload.kind === "ended") {
          current.unlisten?.();
          active.current = null;
          setListening(false);
          return;
        }
        const transcript = payload.text?.trim();
        if (!transcript) return;
        const prefix = current.base.slice(0, current.start);
        const suffix = current.base.slice(current.end);
        const leading = prefix && !/\s$/.test(prefix) ? " " : "";
        const trailing = suffix && !/^\s/.test(suffix) ? " " : "";
        const nextText = `${prefix}${leading}${transcript}${trailing}${suffix}`;
        if (utf8ByteLength(nextText) > MAX_CHAT_COMPOSER_BYTES) {
          setError("Dictation is too long for this message.");
          cancel();
          return;
        }
        change.current(nextText);
        const caret = prefix.length + leading.length + transcript.length;
        queueMicrotask(() => input?.setSelectionRange(caret, caret));
        if (payload.kind === "final") {
          current.base = nextText;
          current.start = caret;
          current.end = caret;
        }
      });
      if (active.current !== current) {
        current.unlisten();
        return;
      }
      await invokeNative("start_voice_input", { id });
      if (active.current !== current) {
        await invokeNative("stop_voice_input", { id, cancel: true });
        return;
      }
      setListening(true);
    } catch (cause) {
      cancel();
      setError(
        cause instanceof Error
          ? cause.message
          : "Dictation could not start. Check microphone access.",
      );
    }
  };
  const stop = () => {
    if (browser.current) {
      browser.current.stop();
      browser.current = null;
      setListening(false);
      return;
    }
    const current = active.current;
    if (current) {
      active.current = null;
      current.unlisten?.();
      void invokeNative("stop_voice_input", {
        id: current.id,
        cancel: false,
      }).catch(() => setError("Dictation could not stop."));
    }
    setListening(false);
  };
  return {
    supported,
    listening: listeningState && !suspended,
    error,
    start,
    stop,
    cancel,
  };
}

export function composerOptionLabel(
  option: AcpConfigOption | undefined,
): string | null {
  return option?.type === "select"
    ? flattenChoices(option).find(
        (choice) => choice.value === option.currentValue,
      )?.name || option.currentValue
    : null;
}

export function composerShortcutLabel(usesMetaModifier: boolean): string {
  return shortcutLabel("l", usesMetaModifier);
}

export function composerPromptPlaceholder(
  canPrompt: boolean,
  sessionPromptUnavailable = false,
): string {
  if (canPrompt !== true) {
    if (sessionPromptUnavailable) return "This session cannot accept prompts.";
    return COMPOSER_PROMPT_PLACEHOLDER_DISABLED;
  }
  return COMPOSER_PROMPT_PLACEHOLDER;
}

export function composerWorkspaceName(
  project: { name: string } | null | undefined,
  maxLength = COMPOSER_WORKSPACE_NAME_CAP,
): string | null {
  if (!project) return null;
  const name = project.name.trim();
  if (
    !name ||
    name.split("").some((character) => {
      const code = character.charCodeAt(0);
      return code < 32 || code === 127;
    })
  ) {
    return null;
  }
  if (name.length <= maxLength) return name;
  return `${name.slice(0, maxLength - 1)}…`;
}

export function isComposerFocusChord(event: KeymapChordEvent): boolean {
  return matchesKeymapChord(keymapBinding("focusComposer"), event);
}

export function useComposerFocusHotkey(
  composerRef: RefObject<HTMLTextAreaElement | null>,
  enabled: boolean,
): void {
  useEffect(() => {
    if (enabled !== true) {
      return;
    }
    const onKeyDown = (event: KeyboardEvent) => {
      if (!isComposerFocusChord(event)) {
        return;
      }
      const composer = composerRef.current;
      if (!composer || composer.disabled) {
        return;
      }
      event.preventDefault();
      composer.focus();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [composerRef, enabled]);
}

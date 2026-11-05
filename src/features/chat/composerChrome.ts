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
import { selectedConfigChoice } from "../../domain/acpSessionConfigControls";
import { invokeNative, isTauriRuntime } from "../../ports/nativeInvoke";
import type {
  createDictation,
  BrowserRecognition,
} from "./composerDictationRuntime";
import { shortcutLabel } from "../../ui/shortcutLabel";

export { shortcutLabel, usesMetaModifier } from "../../ui/shortcutLabel";

export const COMPOSER_PROMPT_PLACEHOLDER = "Do anything";
export const COMPOSER_PROMPT_PLACEHOLDER_DISABLED =
  "Choose an available harness before starting a session";
export const COMPOSER_WORKSPACE_NAME_CAP = 48;

function browserRecognition() {
  const speech = window as Window & {
    SpeechRecognition?: new () => BrowserRecognition;
    webkitSpeechRecognition?: new () => BrowserRecognition;
  };
  return speech.SpeechRecognition ?? speech.webkitSpeechRecognition;
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
  const recording = useRef<ReturnType<typeof createDictation> | null>(null);
  const generation = useRef(0);
  const change = useRef(onTextChange);
  useEffect(() => {
    change.current = onTextChange;
  }, [onTextChange]);
  const cancel = useCallback(() => {
    generation.current += 1;
    recording.current?.cancel();
    recording.current = null;
    setListening(false);
  }, []);
  useEffect(() => {
    if (suspended) queueMicrotask(cancel);
  }, [cancel, suspended]);
  useEffect(() => () => cancel(), [cancel, scope]);
  useEffect(() => {
    if (suspended) return;
    window.addEventListener("blur", cancel);
    return () => window.removeEventListener("blur", cancel);
  }, [cancel, suspended]);
  const nativeSupported = isTauriRuntime() && /win/i.test(navigator.platform);
  const supported = nativeSupported || Boolean(browserRecognition());
  const start = async () => {
    if (!supported || suspended) return;
    cancel();
    const request = generation.current;
    setError(null);
    try {
      const { createDictation } = await import("./composerDictationRuntime");
      if (generation.current !== request) return;
      const current = createDictation({
        sequence: request,
        text,
        input: inputRef.current,
        getText: () => inputRef.current?.value ?? text,
        invoke: invokeNative,
        onText: (next) => change.current(next),
        onError: (message) => {
          setError(message);
          cancel();
        },
        onEnded: () => {
          recording.current = null;
          setListening(false);
        },
      });
      recording.current = current;
      const active = await current.start(
        nativeSupported ? null : (browserRecognition() ?? null),
      );
      if (recording.current === current) setListening(active);
    } catch (cause) {
      if (generation.current !== request) return;
      cancel();
      setError(
        cause instanceof Error
          ? cause.message
          : "Dictation could not start. Check microphone access.",
      );
    }
  };
  const stop = () => {
    generation.current += 1;
    const epoch = generation.current;
    recording.current?.stop((message) => {
      if (generation.current === epoch) setError(message);
    });
    recording.current = null;
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
    ? selectedConfigChoice(option)?.name || option.currentValue
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

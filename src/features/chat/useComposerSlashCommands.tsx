import { useRef, useState, type KeyboardEvent, type RefObject } from "react";
import type * as SlashChunk from "./ComposerSlashController";
import { lazyParts } from "./lazyParts";

const SlashPart = lazyParts<Pick<typeof SlashChunk, "ComposerSlashController">>(
  () => import("./ComposerSlashController"),
);

/**
 * The composer's eager stub for the `/` command menu. The controller, the
 * menu and its styles load with the field's first focus; until then this
 * only keeps Enter from sending a half-typed `/name`.
 */
export function useComposerSlashCommands(input: {
  anchorRef: RefObject<HTMLElement | null>;
  fieldRef: RefObject<HTMLTextAreaElement | null>;
  boundChatId?: string;
  disabled: boolean;
  fieldId: string;
  text: string;
  onTextChange: (text: string) => void;
}) {
  const controlRef = useRef<SlashChunk.SlashControl | null>(null);
  const [armed, setArmed] = useState(false);
  const [{ describedBy, ...aria }, share] =
    useState<SlashChunk.SlashFieldShare>({ "aria-expanded": false });
  const trackCaret = (event: { currentTarget: HTMLTextAreaElement }) =>
    controlRef.current?.trackCaret(event.currentTarget);
  return {
    describedBy,
    fieldProps: { ...aria, onSelect: trackCaret },
    trackCaret,
    onFocus: () => {
      setArmed(true);
      controlRef.current?.setFocused(true);
    },
    onBlur: () => controlRef.current?.setFocused(false),
    /** True when the menu took the key. */
    onKeyDown: (event: KeyboardEvent<HTMLTextAreaElement>) => {
      const control = controlRef.current;
      if (control) return control.onKeyDown(event);
      const hold =
        event.key === "Enter" &&
        !(event.shiftKey || event.altKey || event.ctrlKey || event.metaKey) &&
        /^\/\S*$/u.test(event.currentTarget.value);
      if (hold) event.preventDefault();
      return hold;
    },
    pickedCommandFor: (prompt: string) =>
      controlRef.current?.pickedCommandFor(prompt),
    controller: armed ? (
      <SlashPart
        part="ComposerSlashController"
        {...input}
        controlRef={controlRef}
        onShare={share}
      />
    ) : null,
  };
}

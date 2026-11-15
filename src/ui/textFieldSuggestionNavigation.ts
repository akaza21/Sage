import { useCallback, useMemo, type ReactNode } from "react";
import { useActiveOption } from "./listNavigation";
import { isImeComposing } from "./imeComposition";

/** Rows rendered at once; the rest are counted and reached by typing more. */
export const TEXT_FIELD_SUGGESTIONS_MAX_RENDERED = 50;

export interface TextFieldSuggestion {
  id: string;
  label: string;
  icon?: ReactNode;
  description?: string;
  hint?: string;
  accessibleLabel: string;
  /** Rows that share a group must be contiguous. */
  group?: string;
}

export type TextFieldSuggestionsState =
  | { kind: "ready" }
  | { kind: "loading"; title: string; detail?: string }
  | { kind: "error"; title: string; detail?: string }
  | { kind: "empty"; title: string; detail?: string };

export type SuggestionKeyEvent = Pick<
  KeyboardEvent,
  | "key"
  | "shiftKey"
  | "altKey"
  | "ctrlKey"
  | "metaKey"
  | "isComposing"
  | "keyCode"
  | "preventDefault"
  | "stopPropagation"
>;

export function suggestionOptionId(listboxId: string, id: string) {
  return `${listboxId}-option-${encodeURIComponent(id)}`;
}

export function useSuggestionNavigation(
  candidates: readonly TextFieldSuggestion[],
  onPick: (item: TextFieldSuggestion) => void,
  onDismiss: () => void,
) {
  const items = useMemo(
    () => candidates.slice(0, TEXT_FIELD_SUGGESTIONS_MAX_RENDERED),
    [candidates],
  );
  const ids = useMemo(() => items.map((item) => item.id), [items]);
  const { activeIndex, move, setActiveId } = useActiveOption(ids, "first");
  const active = items[activeIndex] ?? null;
  const onKeyDown = useCallback(
    (event: SuggestionKeyEvent): boolean => {
      if (
        isImeComposing(event) ||
        event.altKey ||
        event.ctrlKey ||
        event.metaKey
      )
        return false;
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        onDismiss();
        return true;
      }
      if (!active) return false;
      if (
        event.key === "ArrowDown" ||
        event.key === "ArrowUp" ||
        event.key === "PageDown" ||
        event.key === "PageUp"
      ) {
        event.preventDefault();
        move(event.key);
        return true;
      }
      if ((event.key === "Enter" || event.key === "Tab") && !event.shiftKey) {
        event.preventDefault();
        onPick(active);
        return true;
      }
      return false;
    },
    [active, move, onDismiss, onPick],
  );
  return { active, setActiveId, onKeyDown };
}

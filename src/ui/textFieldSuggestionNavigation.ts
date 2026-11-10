import { useCallback, useMemo, useState } from "react";
import { isImeComposing } from "./imeComposition";

/** Rows rendered at once; the rest are counted and reached by typing more. */
export const TEXT_FIELD_SUGGESTIONS_MAX_RENDERED = 50;

export interface TextFieldSuggestion {
  id: string;
  label: string;
  description?: string;
  /** Muted trailing text, such as an argument hint. */
  hint?: string;
  /** The whole row as a screen reader should hear it. */
  accessibleLabel: string;
  /**
   * The heading of the section the row belongs to. Rows of one section are
   * contiguous; a list whose rows share one section shows no heading.
   */
  group?: string;
}

export type TextFieldSuggestionsState =
  | { kind: "ready" }
  | { kind: "loading"; title: string; detail?: string }
  | { kind: "error"; title: string; detail?: string }
  | { kind: "empty"; title: string; detail?: string };

/** The keys the list reads from the field's native keydown. */
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

/**
 * Keyboard model for a list that pops up from a text field which keeps the
 * focus (the WAI-ARIA combobox pattern with aria-activedescendant). Up and
 * Down move, Enter or Tab pick, Escape dismisses; every other key edits the
 * field. The first row is active until the user moves.
 */
export function useSuggestionNavigation(
  candidates: readonly TextFieldSuggestion[],
  onPick: (item: TextFieldSuggestion) => void,
  onDismiss: () => void,
) {
  const items = useMemo(
    () => candidates.slice(0, TEXT_FIELD_SUGGESTIONS_MAX_RENDERED),
    [candidates],
  );
  const [chosenId, setChosenId] = useState<string | null>(null);
  const active = items.find((item) => item.id === chosenId) ?? items[0] ?? null;
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
      if (event.key === "ArrowDown" || event.key === "ArrowUp") {
        event.preventDefault();
        const index = items.indexOf(active);
        const step = event.key === "ArrowDown" ? 1 : -1;
        setChosenId(items[(index + step + items.length) % items.length]!.id);
        return true;
      }
      if ((event.key === "Enter" || event.key === "Tab") && !event.shiftKey) {
        event.preventDefault();
        onPick(active);
        return true;
      }
      return false;
    },
    [active, items, onDismiss, onPick],
  );
  return { active, setActiveId: setChosenId, onKeyDown };
}

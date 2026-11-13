import { useLayoutEffect, type RefObject } from "react";

/** Attribute values for the field; null removes one. */
export type FieldAttributes = Record<string, string | null>;

function apply(field: HTMLElement, values: [string, string | null][]) {
  for (const [name, value] of values) {
    if (value === null) field.removeAttribute(name);
    else field.setAttribute(name, value);
  }
}

/**
 * Lets several pop-up lists share one text field's combobox attributes
 * (`aria-controls`, `aria-activedescendant` and the like): while `active`,
 * the field carries what `values` returns for it, and once it is not, or
 * the owner unmounts, the field gets back exactly what it held before.
 * Each owner therefore holds the attributes only while its own list is in
 * play and never clears another's.
 */
export function useOwnedFieldAttributes(
  fieldRef: RefObject<HTMLElement | null>,
  active: boolean,
  values: (field: HTMLElement) => FieldAttributes,
) {
  useLayoutEffect(() => {
    const field = fieldRef.current;
    if (!field || !active) return;
    const next = Object.entries(values(field));
    const held = next.map(([name]): [string, string | null] => [
      name,
      field.getAttribute(name),
    ]);
    apply(field, next);
    // Each render puts back what it found before the next one applies.
    return () => apply(field, held);
  });
}

import { useLayoutEffect, type RefObject } from "react";

export type FieldAttributes = Record<string, string | null>;

function apply(field: HTMLElement, values: [string, string | null][]) {
  for (const [name, value] of values) {
    if (value === null) field.removeAttribute(name);
    else field.setAttribute(name, value);
  }
}

/** Pop-up owners share the field, so each restores what it held before. */
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

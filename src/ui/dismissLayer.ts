import {
  createContext,
  useContext,
  useLayoutEffect,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type RefObject,
} from "react";

import { isImeComposing } from "./imeComposition";

export interface DismissLayerOptions {
  inside: readonly RefObject<Element | null>[];
  onEscape?: () => void;
  onOutside?: (event: PointerEvent) => void;
  modal?: boolean;
}

export type DismissLayer = {
  options: RefObject<DismissLayerOptions>;
  parent: RefObject<DismissLayer | null>;
};
type Layer = DismissLayer;

/**
 * A layer's children read it, so a child layer always stacks above its parent,
 * even when both open together and the child's effects run first.
 */
export const DismissLayerContext = createContext<DismissLayer | null>(null);

const stack: Layer[] = [];

function descends(layer: Layer, ancestor: Layer) {
  for (let up = layer.parent.current; up; up = up.parent.current)
    if (up === ancestor) return true;
  return false;
}

function contains(layer: Layer, target: EventTarget | null) {
  return (
    target instanceof Node &&
    layer.options.current.inside.some((ref) => ref.current?.contains(target))
  );
}

function topmostContaining(target: EventTarget | null) {
  for (let index = stack.length - 1; index >= 0; index -= 1)
    if (contains(stack[index], target)) return index;
  return -1;
}

function onPointerDown(event: PointerEvent) {
  const hit = topmostContaining(event.target);
  // Collect first: a dismissal may close other layers synchronously.
  const outside: Layer[] = [];
  for (let index = stack.length - 1; index > hit; index -= 1) {
    const layer = stack[index];
    if (layer.options.current.onOutside) outside.push(layer);
    if (layer.options.current.modal) break;
  }
  for (const layer of outside)
    if (stack.includes(layer)) layer.options.current.onOutside?.(event);
}

function push(layer: Layer) {
  if (stack.length === 0)
    document.addEventListener("pointerdown", onPointerDown, true);
  const child = stack.findIndex((entry) => descends(entry, layer));
  if (child < 0) stack.push(layer);
  else stack.splice(child, 0, layer);
}

function remove(layer: Layer) {
  const index = stack.indexOf(layer);
  if (index < 0) return;
  stack.splice(index, 1);
  if (stack.length === 0)
    document.removeEventListener("pointerdown", onPointerDown, true);
}

/**
 * React bubbles keys through portals along the component tree, so the innermost
 * open layer sees Escape first.
 */
export function dismissLayerKeyDown(
  event: ReactKeyboardEvent,
  onEscape: (() => void) | undefined,
) {
  if (
    !onEscape ||
    event.key !== "Escape" ||
    event.defaultPrevented ||
    isImeComposing(event)
  )
    return;
  // The key belongs to this layer alone: not to the layers or panes around
  // it, and not to a native <dialog>'s cancel.
  event.preventDefault();
  event.stopPropagation();
  onEscape();
}

export function useDismissLayer(open: boolean, options: DismissLayerOptions) {
  const parent = useContext(DismissLayerContext);
  const optionsRef = useRef(options);
  const parentRef = useRef(parent);
  useLayoutEffect(() => {
    optionsRef.current = options;
    parentRef.current = parent;
  });
  const [layer] = useState<Layer>(() => ({
    options: optionsRef,
    parent: parentRef,
  }));
  // Layout timing: a layer opened by a click is in the stack before the next
  // press can reach it.
  useLayoutEffect(() => {
    if (!open) return;
    push(layer);
    return () => remove(layer);
  }, [layer, open]);
  return {
    layer,
    onKeyDown: (event: ReactKeyboardEvent) =>
      dismissLayerKeyDown(event, layer.options.current.onEscape),
  };
}

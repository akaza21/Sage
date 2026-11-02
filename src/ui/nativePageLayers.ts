import { create } from "zustand";
import { useLayoutEffect, type RefObject } from "react";

const MAX_NATIVE_PAGE_LAYERS = 32;
const layerState = create(() => ({
  layers: [] as HTMLElement[],
  overflow: 0,
}));

export const nativePageLayers = layerState.getState;
export const subscribeNativePageLayers = layerState.subscribe;

/** Native child windows sit above DOM layers; only intersecting layers cover them. */
export function nativePageOverlapped(page: DOMRectReadOnly): boolean {
  const { layers, overflow } = nativePageLayers();
  return (
    overflow > 0 ||
    layers.some((layer) => {
      const rect = layer.getBoundingClientRect();
      return (
        getComputedStyle(layer).visibility !== "hidden" &&
        rect.width > 0 &&
        rect.height > 0 &&
        rect.left < page.right &&
        rect.right > page.left &&
        rect.top < page.bottom &&
        rect.bottom > page.top
      );
    })
  );
}

function registerNativePageLayer(element: HTMLElement) {
  const admitted = layerState.getState().layers.length < MAX_NATIVE_PAGE_LAYERS;
  layerState.setState((state) =>
    admitted
      ? { layers: [...state.layers, element] }
      : { overflow: state.overflow + 1 },
  );
  return () => {
    layerState.setState((state) =>
      admitted
        ? { layers: state.layers.filter((layer) => layer !== element) }
        : { overflow: state.overflow - 1 },
    );
  };
}

/** Register only open UI layers; native pages never scan the document. */
export function useNativePageLayer(
  ref: RefObject<HTMLElement | null>,
  open = true,
  portalRoot?: HTMLElement,
) {
  useLayoutEffect(() => {
    const layer = ref.current;
    if (!open || !layer) return;
    const unregister = registerNativePageLayer(layer);
    const changed = () => layerState.setState({});
    const resize =
      typeof ResizeObserver === "undefined"
        ? null
        : new ResizeObserver(changed);
    resize?.observe(layer);
    const mutation = new MutationObserver(changed);
    mutation.observe(layer, {
      attributes: true,
      attributeFilter: ["hidden", "open", "style", "class"],
    });
    return () => {
      resize?.disconnect();
      mutation.disconnect();
      unregister();
    };
  }, [ref, open, portalRoot]);
}

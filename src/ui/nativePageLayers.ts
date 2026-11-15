import { create } from "zustand";
import { useLayoutEffect, type RefObject } from "react";

const MAX_NATIVE_PAGE_LAYERS = 32;
type Coverage = "overlap" | "window";
const layerState = create(() => ({
  layers: [] as { element: HTMLElement; coverage: Coverage }[],
  overflow: 0,
}));

export const nativePageLayers = layerState.getState;
export const subscribeNativePageLayers = layerState.subscribe;

export function nativePageOverlapped(page: DOMRectReadOnly): boolean {
  const { layers, overflow } = nativePageLayers();
  return (
    overflow > 0 ||
    layers.some(({ element, coverage }) => {
      if (coverage === "window") return true;
      const rect = element.getBoundingClientRect();
      return (
        getComputedStyle(element).visibility !== "hidden" &&
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

function registerNativePageLayer(element: HTMLElement, coverage: Coverage) {
  const admitted = layerState.getState().layers.length < MAX_NATIVE_PAGE_LAYERS;
  layerState.setState((state) =>
    admitted
      ? { layers: [...state.layers, { element, coverage }] }
      : { overflow: state.overflow + 1 },
  );
  return () => {
    layerState.setState((state) =>
      admitted
        ? { layers: state.layers.filter((layer) => layer.element !== element) }
        : { overflow: state.overflow - 1 },
    );
  };
}

export function useNativePageLayer(
  ref: RefObject<HTMLElement | null>,
  {
    open = true,
    portalRoot,
    coverage = "overlap",
  }: {
    open?: boolean;
    portalRoot?: HTMLElement;
    coverage?: Coverage;
  } = {},
) {
  useLayoutEffect(() => {
    const layer = ref.current;
    if (!open || !layer) return;
    const unregister = registerNativePageLayer(layer, coverage);
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
  }, [ref, open, portalRoot, coverage]);
}

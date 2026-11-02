import { create } from "zustand";

const useModalDialogState = create(() => ({ count: 0 }));

export function useModalDialogOpen() {
  return useModalDialogState((state) => state.count > 0);
}

export function registerModalDialog() {
  useModalDialogState.setState((state) => ({ count: state.count + 1 }));
  return () => {
    useModalDialogState.setState((state) => ({ count: state.count - 1 }));
  };
}

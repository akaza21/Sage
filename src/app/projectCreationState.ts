import { create } from "zustand";

// Only the open intent survives workbench replacement; the mounted dialog owns
// its bounded form state and releases it on dismissal.
export const useProjectCreationState = create(() => ({ open: false }));

export const openProjectCreation = () =>
  useProjectCreationState.setState({ open: true });
export const closeProjectCreation = () =>
  useProjectCreationState.setState({ open: false });

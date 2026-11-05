import type { ReactNode } from "react";

export type TerminalDockProjection = Readonly<{
  visible: boolean;
  controls: ReactNode;
  setOpen: (open: boolean) => void;
  open: () => void;
}>;

const ignore = () => undefined;
export const DISCONNECTED_TERMINAL: TerminalDockProjection = {
  visible: false,
  controls: null,
  setOpen: ignore,
  open: ignore,
};

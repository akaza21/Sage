import { useEffect } from "react";
import type { HarnessSignInDecoration } from "./harnessSignIn";
import { useHarnessSignInDecorations } from "./harnessSignInBridge";

export function HarnessSignInStatus({
  harnesses,
  onChange,
}: {
  harnesses: readonly { id: string; name: string; installState: string }[];
  onChange: (decorations: ReadonlyMap<string, HarnessSignInDecoration>) => void;
}) {
  const decorations = useHarnessSignInDecorations(harnesses);
  useEffect(() => onChange(decorations), [decorations, onChange]);
  return null;
}

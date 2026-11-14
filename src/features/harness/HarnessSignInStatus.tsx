import { useEffect, useState } from "react";
import { type HarnessSignInDecoration } from "./harnessSignIn";
import { isTauriRuntime } from "../../ports/desktopRuntime";

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

/** Keeps the harness sign-in module out of the initial Agent bundle. */
function useHarnessSignInDecorations(
  harnessDefinitions: readonly {
    id: string;
    name: string;
    installState: string;
  }[],
  enabled = isTauriRuntime(),
): ReadonlyMap<string, HarnessSignInDecoration> {
  const harnesses = harnessDefinitions.filter(
    (harness) => harness.installState === "available",
  );
  const [decorations, setDecorations] = useState<
    ReadonlyMap<string, HarnessSignInDecoration>
  >(() => new Map());
  const key = harnesses
    .map((harness) => `${harness.id}\0${harness.name}`)
    .join("\n");
  useEffect(() => {
    if (!enabled || !key) return;
    let active = true;
    let unsubscribe: (() => void) | undefined;
    const entries = key.split("\n").map((entry) => {
      const [id, name] = entry.split("\0");
      return { id, name };
    });
    void import("./harnessSignIn").then((signIn) => {
      if (!active) return;
      const publish = () => {
        if (active) setDecorations(signIn.harnessSignInDecorations(entries));
      };
      unsubscribe = signIn.subscribeHarnessSignIn(publish);
      publish();
      signIn.ensureHarnessSignInChecked(entries.map((entry) => entry.id));
    });
    return () => {
      active = false;
      unsubscribe?.();
    };
  }, [enabled, key]);
  return decorations;
}

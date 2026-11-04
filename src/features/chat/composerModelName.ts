import { useMemo } from "react";

export function useComposerModelName(name: string): string {
  return useMemo(() => composerModelName(name), [name]);
}

export function composerModelName(name: string): string {
  const claude =
    /^claude[- ](sonnet|opus|haiku)[- ](\d+)(?:[- .](\d{1,2}))?(?:[- ]\d{8})?$/i.exec(
      name,
    );
  if (claude) {
    const family =
      claude[1][0].toUpperCase() + claude[1].slice(1).toLowerCase();
    return (
      "Claude " + family + " " + claude[2] + (claude[3] ? "." + claude[3] : "")
    );
  }
  const gpt = /^gpt[- ](\d+(?:\.\d+)?)(?:[- ](.+))?$/i.exec(name);
  return gpt
    ? "GPT-" +
        gpt[1] +
        (gpt[2]
          ? " " +
            gpt[2]
              .replace(/-/g, " ")
              .replace(/\b[a-z]/g, (letter) => letter.toUpperCase())
          : "")
    : name;
}

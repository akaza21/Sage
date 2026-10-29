import { useState, type ComponentProps } from "react";

import { SearchablePicker } from "../../ui/SearchablePicker";
import { HarnessSignInPart } from "../chat/lazyParts";
import type { HarnessSignInDecoration } from "./harnessSignIn";

type Props = ComponentProps<typeof SearchablePicker> & {
  harnesses: readonly { id: string; name: string; installState: string }[];
};

export function HarnessSignInPicker({
  harnesses,
  candidates,
  ...props
}: Props) {
  const [signIn, setSignIn] = useState<
    ReadonlyMap<string, HarnessSignInDecoration>
  >(() => new Map());
  return (
    <>
      <SearchablePicker
        {...props}
        candidates={candidates.map((row) => {
          const decoration = signIn.get(row.id);
          return decoration
            ? {
                ...row,
                subtitle: [row.subtitle, decoration.label]
                  .filter(Boolean)
                  .join(" ? "),
                accessibleLabel: `${row.accessibleLabel ?? row.label}. ${decoration.label}`,
                action: decoration.action,
              }
            : row;
        })}
      />
      <HarnessSignInPart
        part="HarnessSignInStatus"
        harnesses={harnesses}
        onChange={setSignIn}
      />
    </>
  );
}

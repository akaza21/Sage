import type { HarnessDefinition } from "../../domain/harness";
import { CodexAccountSettings } from "./CodexAccountSettings";

export default function HarnessAccountDetails({
  harness,
  selected,
  available,
  disabled,
}: {
  harness: HarnessDefinition;
  selected: boolean;
  available: boolean;
  disabled: boolean;
}) {
  return (
    <>
      {selected && harness.accountModelDiscovery && available ? (
        <CodexAccountSettings disabled={disabled} />
      ) : null}
      {harness.update ? (
        <p>
          Update available: {harness.version} → {harness.update.latestVersion}.
          Run <code>{harness.update.command}</code> in a terminal, then choose
          Refresh discovery.
        </p>
      ) : null}
      {harness.signIn && available ? (
        <p>
          {harness.signIn.command ? (
            <>
              Run <code>{harness.signIn.command}</code> in a terminal.{" "}
            </>
          ) : null}
          {harness.signIn.hint}
        </p>
      ) : null}
    </>
  );
}

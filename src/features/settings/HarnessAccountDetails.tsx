import type { HarnessDefinition } from "../../domain/harness";
import { CodexAccountSettings } from "./CodexAccountSettings";
import { HarnessVersionDetails } from "./HarnessVersions";

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
      <HarnessVersionDetails harness={harness} />
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

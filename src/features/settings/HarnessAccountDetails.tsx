import type { HarnessDefinition } from "../../domain/harness";
import { CodexAccountSettings } from "./CodexAccountSettings";
import { HarnessVersionDetails } from "./HarnessVersions";

export default function HarnessAccountDetails({
  harness,
  selected,
  available,
  showSignInHint,
  disabled,
}: {
  harness: HarnessDefinition;
  selected: boolean;
  available: boolean;
  /** False once the harness reports a signed-in account. */
  showSignInHint: boolean;
  disabled: boolean;
}) {
  return (
    <>
      {selected && harness.accountModelDiscovery && available ? (
        <CodexAccountSettings disabled={disabled} />
      ) : null}
      <HarnessVersionDetails harness={harness} />
      {harness.signIn && available && showSignInHint ? (
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

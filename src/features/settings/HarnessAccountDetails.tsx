import type { HarnessDefinition } from "../../domain/harness";
import { LazyModuleBoundary } from "../../ui/LazyModuleBoundary";
import { CodexAccountSettings } from "./CodexAccountSettings";

// One chunk with the npm switch, loaded only on Settings › Harnesses.
const loadVersionDetails = () =>
  import("./HarnessVersions").then(({ HarnessVersionDetails }) => ({
    default: HarnessVersionDetails,
  }));

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
      <LazyModuleBoundary
        label="Harness versions"
        fallback={null}
        load={loadVersionDetails}
        render={(Versions) => <Versions harness={harness} />}
      />
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

import type { HarnessDefinition } from "../../domain/harness";

export function SignInRequiredMessage({
  harness: { name, signIn },
}: {
  harness: Pick<HarnessDefinition, "name" | "signIn">;
}) {
  const guidance = signIn
    ? `${signIn.command ? `Run ${signIn.command}. ` : ""}${signIn.hint} Then try again.`
    : "Sign in with its CLI, then try again.";
  return `${name} requires sign-in before it can continue. ${guidance}`;
}

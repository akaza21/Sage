import type { HarnessDefinition } from "../../domain/harness";

export function SignInRequiredMessage({
  harness: { name, signIn },
}: {
  harness: Pick<HarnessDefinition, "name" | "signIn">;
}) {
  if (!signIn)
    return `${name} requires sign-in before it can continue. Sign in with its CLI, then try again.`;
  const run = signIn.command ? `Run ${signIn.command}. ` : "";
  return `${name} requires sign-in before it can continue. ${run}${signIn.hint} Then try again.`;
}

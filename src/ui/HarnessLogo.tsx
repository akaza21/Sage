import "../styles/harness-logo.css";

export type HarnessLogoKind =
  "claude" | "codex" | "pi" | "opencode" | "generic";

/**
 * A harness brand mark. The mark is a CSS mask over the current text colour,
 * so monochrome marks follow the theme and forced colours, and brand-coloured
 * marks take their colour from a token.
 */
export function HarnessLogo({ kind }: { kind: HarnessLogoKind }) {
  return (
    <span
      className="harness-logo"
      data-harness-icon={kind}
      aria-hidden="true"
    />
  );
}

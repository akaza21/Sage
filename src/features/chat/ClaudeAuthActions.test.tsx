import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { SignInRequiredMessage } from "./ClaudeAuthActions";

describe("SignInRequiredMessage", () => {
  it("names the command only when the harness has one", () => {
    const { container, rerender } = render(
      <SignInRequiredMessage
        harness={{
          name: "Team agent",
          signIn: { command: "", hint: "It offers: Browser." },
        }}
      />,
    );
    expect(container).toHaveTextContent(
      "Team agent requires sign-in before it can continue. It offers: Browser. Then try again.",
    );
    rerender(<SignInRequiredMessage harness={{ name: "Pi", signIn: null }} />);
    expect(container).toHaveTextContent(
      "Pi requires sign-in before it can continue. Sign in with its CLI, then try again.",
    );
  });
});

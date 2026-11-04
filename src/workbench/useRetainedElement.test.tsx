import { act, cleanup, render } from "@testing-library/react";
import { Activity } from "react";
import { afterEach, expect, it } from "vitest";

import { useRetainedElement } from "../ui/useRetainedElement";

afterEach(cleanup);

function Host({
  hidden,
  mounted = true,
}: {
  hidden: boolean;
  mounted?: boolean;
}) {
  const [element, ref] = useRetainedElement<HTMLDivElement>();
  return (
    <>
      <output data-testid="retained">{element?.dataset.slot ?? "none"}</output>
      {mounted ? (
        <Activity mode={hidden ? "hidden" : "visible"}>
          <div ref={ref} data-slot="slot" />
        </Activity>
      ) : null}
    </>
  );
}

const flushMicrotasks = () => act(async () => await Promise.resolve());

it("keeps the element while Activity hides it", async () => {
  const view = render(<Host hidden={false} />);
  const retained = view.getByTestId("retained");
  expect(retained).toHaveTextContent("slot");

  view.rerender(<Host hidden />);
  await flushMicrotasks();
  expect(retained).toHaveTextContent("slot");

  view.rerender(<Host hidden={false} />);
  await flushMicrotasks();
  expect(retained).toHaveTextContent("slot");
});

it("drops the element once it really unmounts", async () => {
  const view = render(<Host hidden={false} />);
  expect(view.getByTestId("retained")).toHaveTextContent("slot");

  view.rerender(<Host hidden={false} mounted={false} />);
  await flushMicrotasks();
  expect(view.getByTestId("retained")).toHaveTextContent("none");
});

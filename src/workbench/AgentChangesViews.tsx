import { lazy, Suspense } from "react";

const ChangesSurface = lazy(() =>
  import("../features/diff/ChangesSurface").then((module) => ({
    default: module.ChangesSurface,
  })),
);
const ChangesTab = lazy(() =>
  import("../features/diff/ChangesTab").then((module) => ({
    default: module.ChangesTab,
  })),
);

export function AgentChangesView({ chatId }: { chatId: string | null }) {
  return (
    <Suspense fallback={<p role="status">Loading changes…</p>}>
      <ChangesSurface chatId={chatId} />
    </Suspense>
  );
}

export function AgentChangesTab(props: {
  selected: boolean;
  select: () => void;
  close: () => void;
}) {
  return (
    <Suspense fallback={null}>
      <ChangesTab {...props} />
    </Suspense>
  );
}

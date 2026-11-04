import { useState } from "react";
import type { AcpChatPage } from "../domain/acp";
import { lazyParts } from "../features/chat/lazyParts";

const ObserverPart = lazyParts(
  () => import("../features/diff/AgentChangesObserver"),
);

export function AgentChangesObserver(props: {
  page: AcpChatPage | null;
  generation: number;
}) {
  const [initialPage] = useState(props.page);
  return initialPage && props.page && props.page !== initialPage ? (
    <ObserverPart
      part="AgentChangesObserver"
      {...props}
      page={props.page}
      initialPage={initialPage}
    />
  ) : null;
}

const SurfacePart = lazyParts(() => import("../features/diff/ChangesSurface"));
const TabPart = lazyParts(() => import("../features/diff/ChangesTab"));

export function AgentChangesView({ chatId }: { chatId: string | null }) {
  return (
    <SurfacePart
      part="ChatChangesSurface"
      chatId={chatId}
      fallback={<p role="status">Loading changes…</p>}
    />
  );
}

export function AgentChangesTab(props: {
  selected: boolean;
  select: () => void;
  close: () => void;
}) {
  return <TabPart part="ChangesTab" {...props} />;
}

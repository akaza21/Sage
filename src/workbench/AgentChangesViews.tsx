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
  const turn = props.page?.turns.at(-1);
  const turnKey = `${turn?.id}:${turn?.state}`;
  const [initialTurnKey] = useState(turnKey);
  const [observing, setObserving] = useState(false);
  if (!observing && turnKey !== initialTurnKey) setObserving(true);
  return observing && props.page ? (
    <ObserverPart
      part="AgentChangesObserver"
      {...props}
      page={props.page}
      initialTurnKey={initialTurnKey}
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

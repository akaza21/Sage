import { useContext, useId, useState, type ComponentProps } from "react";
import { Ellipsis } from "../../ui/leanIcons";
import { AnswerActionButton } from "./AnswerActions";
import { ComposerMenuLoading } from "./ComposerMenuLoading";
import { lazyParts } from "./lazyParts";
import type { AnswerMoreActions } from "./ForkChat";
import type * as ConversationChunk from "./ConversationActions";
import type { AcpChatPage, AcpItem, TurnProjection } from "../../domain/acp";
import {
  conversationActions,
  conversationState,
} from "./conversationActionAvailability";
import { PromptPageContext } from "./acp/TurnPrompts";

const ConversationPart = lazyParts<typeof ConversationChunk>(
  () => import("./ConversationActions"),
);

type ConversationViewProps = {
  [K in keyof typeof ConversationChunk]: { view: K } & ComponentProps<
    (typeof ConversationChunk)[K]
  >;
}[keyof typeof ConversationChunk];

export function ConversationView({ view, ...props }: ConversationViewProps) {
  return (
    <ConversationPart
      part={view}
      {...props}
      onLoadFailed={"onClose" in props ? props.onClose : undefined}
    />
  );
}

// Every prompt re-checks on each page update; fold each page once. Weak keys
// retain nothing after a page is replaced.
const folded = new WeakMap<AcpChatPage, ReturnType<typeof conversationState>>();
const foldedState = (page: AcpChatPage) => {
  let state = folded.get(page);
  if (!state) folded.set(page, (state = conversationState(page)));
  return state;
};

/** The trigger shows only when its menu would offer rewind or fork. */
export function PromptActions({
  item,
  turn,
}: {
  item: AcpItem;
  turn: TurnProjection;
}) {
  const page = useContext(PromptPageContext);
  const actions = page && conversationActions(page, turn, foldedState(page));
  return item.kind === "text" &&
    item.payload.role === "user" &&
    actions &&
    (actions.rewind || actions.fork) ? (
    <MessageActionsTrigger page={page} turn={turn} text={item.payload.text} />
  ) : null;
}

/** Row chrome shares the already visible transcript controls; details load on open. */
export function MessageActionsTrigger(
  props: Omit<
    ComponentProps<typeof AnswerMoreActions>,
    "trigger" | "onDismiss" | "menuId"
  >,
) {
  const menuId = useId();
  const [trigger, setTrigger] = useState<HTMLButtonElement | null>(null);
  const close = () => setTrigger(null);
  const content = (
    <>
      <AnswerActionButton
        label={props.turn ? "Message actions" : "More actions"}
        aria-haspopup="menu"
        aria-expanded={!!trigger}
        aria-controls={trigger ? menuId : undefined}
        data-fork-trigger={props.forkable || undefined}
        onClick={(event) => setTrigger(trigger ? null : event.currentTarget)}
      >
        <Ellipsis size={14} aria-hidden="true" />
      </AnswerActionButton>
      {trigger ? (
        <ConversationPart
          part="AnswerMoreActions"
          {...props}
          trigger={{ current: trigger }}
          menuId={menuId}
          onDismiss={close}
          onLoadFailed={close}
          fallback={
            <ComposerMenuLoading
              triggerRef={{ current: trigger }}
              onDismiss={close}
            />
          }
        />
      ) : null}
    </>
  );
  return props.turn ? (
    <div
      className="chat-transcript-actions acp-prompt-actions"
      role="group"
      aria-label="Prompt actions"
    >
      {content}
    </div>
  ) : (
    content
  );
}

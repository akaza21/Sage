import { useId, useState, type ComponentProps } from "react";
import { Ellipsis } from "../../ui/leanIcons";
import { AnswerActionButton } from "./AnswerActions";
import { ComposerMenuLoading } from "./ComposerMenuLoading";
import { lazyParts } from "./lazyParts";
import type { AnswerMoreActions } from "./ForkChat";
import type * as ConversationChunk from "./ConversationActions";
import type { AcpItem } from "../../domain/acp";

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

export function PromptActions({
  item,
  interactive,
  ...props
}: ComponentProps<typeof MessageActionsTrigger> & {
  item: Extract<AcpItem, { kind: "text" }>;
  interactive: boolean;
}) {
  return interactive &&
    props.turn?.terminalAt != null &&
    props.turn.conversationTarget ? (
    <MessageActionsTrigger {...props} text={item.payload.text} />
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
        label="More actions"
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
      className="chat-transcript-actions"
      role="group"
      aria-label="Prompt actions"
    >
      {content}
    </div>
  ) : (
    content
  );
}

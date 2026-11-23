import {
  useEffect,
  useRef,
  type ComponentProps,
  type FormEvent,
  useState,
} from "react";
import { useWorkbenchStore } from "../app/store";
import { Check, X } from "../ui/leanIcons";
import { MAX_CHAT_TITLE_BYTES } from "../domain/chatTitle";
import type { AgentResourcesPopover as ResourcesPopover } from "./AgentResourcesPopover";
import { lazyParts } from "../features/chat/lazyParts";
import type * as ResourcesChunk from "./AgentResourcesPopover";

export function AgentTaskRenameForm({
  id,
  title,
  chatId,
  projectId,
  disabled,
  onCancel,
}: {
  id: string;
  title: string;
  chatId: string;
  projectId: string;
  disabled: boolean;
  onCancel: () => void;
}) {
  const renameChat = useWorkbenchStore((state) => state.renameChat);
  const [value, setValue] = useState(title);
  const [error, setError] = useState<string | null>(null);
  async function onSubmit(event: FormEvent) {
    event.preventDefault();
    setError(null);
    if (await renameChat(chatId, value, projectId)) onCancel();
    else
      setError(
        useWorkbenchStore.getState().error ?? "Chat rename could not be saved",
      );
  }
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => {
    input.current?.focus();
  }, []);
  return (
    <form id={id} className="agent-task-header__rename" onSubmit={onSubmit}>
      <input
        ref={input}
        aria-label="New chat name"
        aria-invalid={error ? true : undefined}
        maxLength={MAX_CHAT_TITLE_BYTES}
        value={value}
        disabled={disabled}
        onChange={(event) => setValue(event.currentTarget.value)}
        onKeyDown={(event) => {
          if (event.key === "Escape") {
            event.preventDefault();
            onCancel();
          }
        }}
      />
      <button type="submit" aria-label="Save chat name" disabled={disabled}>
        <Check size={14} aria-hidden="true" />
      </button>
      <button
        type="button"
        aria-label="Cancel chat rename"
        disabled={disabled}
        onClick={onCancel}
      >
        <X size={14} aria-hidden="true" />
      </button>
      {error ? <span role="alert">{error}</span> : null}
    </form>
  );
}

const ResourcesPart = lazyParts<typeof ResourcesChunk>(
  () => import("./AgentResourcesPopover"),
);

export function AgentTaskResources(
  props: ComponentProps<typeof ResourcesPopover>,
) {
  return <ResourcesPart {...props} part="AgentResourcesPopover" />;
}

AgentTaskResources.preload = ResourcesPart.preload;

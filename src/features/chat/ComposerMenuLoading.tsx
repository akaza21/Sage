import {
  lazy,
  Suspense,
  useEffect,
  type ComponentType,
  type RefObject,
} from "react";

interface MenuProps {
  triggerRef: RefObject<HTMLButtonElement | null>;
  onDismiss: () => void;
}

export function ComposerMenuLoading({ triggerRef, onDismiss }: MenuProps) {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape" && event.key !== "Tab") return;
      onDismiss();
      if (event.key === "Escape") {
        event.preventDefault();
        triggerRef.current?.focus();
      }
    };
    const onPointerDown = (event: PointerEvent) => {
      if (!triggerRef.current?.contains(event.target as Node)) onDismiss();
    };
    document.addEventListener("keydown", onKeyDown);
    document.addEventListener("pointerdown", onPointerDown);
    return () => {
      document.removeEventListener("keydown", onKeyDown);
      document.removeEventListener("pointerdown", onPointerDown);
    };
  }, [onDismiss, triggerRef]);
  return (
    <span className="sr-only" role="status">
      Loading options…
    </span>
  );
}

/**
 * A composer menu loaded on first open. Until it arrives, its trigger still
 * dismisses it on Escape, Tab or an outside press.
 */
export function lazyComposerMenu<Props extends MenuProps>(
  load: () => Promise<{ default: ComponentType<Props> }>,
) {
  const Menu = lazy(load);
  return function LazyComposerMenu(props: Props) {
    return (
      <Suspense
        fallback={
          <ComposerMenuLoading
            triggerRef={props.triggerRef}
            onDismiss={props.onDismiss}
          />
        }
      >
        <Menu {...props} />
      </Suspense>
    );
  };
}

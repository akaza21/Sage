import { GitCompareArrows, X } from "../../ui/leanIcons";

export function ChangesTab({
  selected,
  select,
  close,
}: {
  selected: boolean;
  select: () => void;
  close: () => void;
}) {
  return (
    <div
      className={`tab ${selected ? "tab--active" : ""}`}
      role="presentation"
      data-resource-kind="changes"
    >
      <button
        className="tab__activate"
        type="button"
        role="tab"
        aria-selected={selected}
        aria-label="Changes"
        onClick={select}
      >
        <GitCompareArrows size={16} aria-hidden="true" />
        <span className="tab__basename">Changes</span>
      </button>
      <button
        className="tab__close"
        type="button"
        aria-label="Close Changes"
        onClick={close}
      >
        <X size={14} aria-hidden="true" />
      </button>
    </div>
  );
}

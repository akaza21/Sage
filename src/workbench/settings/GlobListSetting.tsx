import { type FormEvent, useLayoutEffect, useRef, useState } from "react";

import {
  type EditorSettingKey,
  type GlobsSettingSpec,
  MAX_GLOBS,
} from "../../domain/editorSettingsCatalogue";
import { EDITOR_SETTING_COPY } from "../../domain/editorSettingsCopy";
import { MAX_PATH_GLOB_BYTES, validPathGlob } from "../../domain/pathGlob";
import { utf8ByteLength } from "../../domain/text";
import { X } from "../../ui/leanIcons";

/** Why a new pattern cannot be added, in the store's own terms; null if it can. */
function patternProblem(pattern: string, patterns: readonly string[]) {
  if (pattern === "") return "Enter a pattern.";
  if (patterns.includes(pattern)) return "That pattern is already listed.";
  if (utf8ByteLength(pattern) > MAX_PATH_GLOB_BYTES) {
    return `Patterns are at most ${MAX_PATH_GLOB_BYTES} bytes.`;
  }
  if (!validPathGlob(pattern)) {
    return "Write the pattern relative to the Project, such as **/dist, without a leading or trailing slash, backslashes, colons, or . and .. folders.";
  }
  return null;
}

/**
 * A list of glob patterns, as in VS Code's `files.exclude` editor: each
 * pattern with a remove button, then a field to add one.
 */
export function GlobListSetting({
  spec,
  id,
  patterns,
  disabled,
  onChange,
}: {
  spec: GlobsSettingSpec;
  id: string;
  patterns: readonly string[];
  disabled: boolean;
  onChange: (patterns: readonly string[]) => void;
}) {
  const copy = EDITOR_SETTING_COPY[spec.key as EditorSettingKey];
  const [draft, setDraft] = useState("");
  const [problem, setProblem] = useState<string | null>(null);
  const list = useRef<HTMLUListElement>(null);
  const input = useRef<HTMLInputElement>(null);
  // After a removal, focus moves to the pattern that took its place, or the
  // field when none is left, instead of falling back to the page.
  const focusAfterRemove = useRef<number | null>(null);
  useLayoutEffect(() => {
    const index = focusAfterRemove.current;
    if (index === null) return;
    focusAfterRemove.current = null;
    const buttons = list.current?.querySelectorAll("button") ?? [];
    (buttons[Math.min(index, buttons.length - 1)] ?? input.current)?.focus();
  }, [patterns]);
  const full = patterns.length >= MAX_GLOBS;
  const remove = (index: number) => {
    focusAfterRemove.current = index;
    onChange(patterns.filter((_, at) => at !== index));
  };
  const add = (event: FormEvent) => {
    event.preventDefault();
    const pattern = draft.trim();
    const found = patternProblem(pattern, patterns);
    setProblem(found);
    if (found || full) return;
    onChange([...patterns, pattern]);
    setDraft("");
  };
  const hint = full ? `The list holds at most ${MAX_GLOBS} patterns.` : problem;
  return (
    <div className="settings-surface__row settings-surface__row--list">
      <div className="settings-surface__copy">
        <span id={`${id}-label`}>{copy.label}</span>
        <p id={`${id}-description`}>{copy.description}</p>
        <small id={`${id}-default`}>Default: {spec.default.join(", ")}</small>
      </div>
      <div className="settings-surface__list">
        <ul ref={list} aria-labelledby={`${id}-label`}>
          {patterns.map((pattern, index) => (
            <li key={`${index}:${pattern}`}>
              <code>{pattern}</code>
              <button
                className="icon-button"
                type="button"
                aria-label={`Remove ${pattern}`}
                disabled={disabled}
                onClick={() => remove(index)}
              >
                <X size={14} aria-hidden="true" />
              </button>
            </li>
          ))}
        </ul>
        {patterns.length === 0 ? (
          <p className="settings-surface__list-empty">No patterns.</p>
        ) : null}
        <form className="settings-surface__list-add" onSubmit={add}>
          <input
            ref={input}
            className="input"
            id={id}
            value={draft}
            placeholder="**/dist"
            spellCheck={false}
            // Read-only rather than disabled when full, so focus stays here.
            disabled={disabled}
            readOnly={full}
            aria-label={`Add to ${copy.label}`}
            aria-invalid={problem ? true : undefined}
            aria-describedby={`${id}-description ${id}-hint`}
            onChange={(event) => {
              setDraft(event.target.value);
              setProblem(null);
            }}
          />
          <button
            className="button button--compact"
            type="submit"
            disabled={disabled || full}
          >
            Add Pattern
          </button>
        </form>
        {/* Always present, so assistive technology announces new text. */}
        <small id={`${id}-hint`} aria-live="polite">
          {hint}
        </small>
      </div>
    </div>
  );
}

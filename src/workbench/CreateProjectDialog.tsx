import { useId, useRef, useState, type FormEvent } from "react";

import { useWorkbenchStore } from "../app/store";
import { errorMessage } from "../domain/errors";
import {
  MAX_PROJECT_NAME_CHARS,
  MAX_PROJECT_SOURCE_FOLDERS,
  type ProjectSourceFolder,
} from "../domain/workbench";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import { ModalDialog } from "../ui/ModalDialog";
import { Folder, FolderPlus, Laptop, X } from "../ui/leanIcons";
import "./create-project-dialog.css";

const fullMessage = `A project holds at most ${MAX_PROJECT_SOURCE_FOLDERS} folders`;

/** The rail's "+": name a project and pick the folders it works in. */
export function CreateProjectDialog({ onClose }: { onClose: () => void }) {
  const [name, setName] = useState("");
  const [folders, setFolders] = useState<ProjectSourceFolder[]>([]);
  const [picking, setPicking] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const nameRef = useRef<HTMLInputElement>(null);
  const titleId = useId();
  const errorId = useId();
  const createProject = useWorkbenchStore((state) => state.createProject);
  // An unnamed project takes its first folder's name.
  const projectName =
    name.trim() || folders[0]?.name.slice(0, MAX_PROJECT_NAME_CHARS) || "";
  const busy = picking || saving;
  const full = folders.length >= MAX_PROJECT_SOURCE_FOLDERS;

  const addFolder = async () => {
    setPicking(true);
    setError(null);
    try {
      const folder =
        await getWorkbenchPorts().project.pickProjectSourceFolder();
      if (!folder) return;
      if (folders.some((held) => held.path === folder.path)) {
        setError(`${folder.name} is already a source folder.`);
        return;
      }
      setFolders((held) => [...held, folder]);
    } catch (pickError) {
      setError(errorMessage(pickError));
    } finally {
      setPicking(false);
    }
  };

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (!projectName || busy) return;
    setSaving(true);
    setError(null);
    const committed = await createProject({
      name: projectName,
      sourceTokens: folders.map((folder) => folder.token),
    });
    setSaving(false);
    if (committed) {
      onClose();
      return;
    }
    setError(
      useWorkbenchStore.getState().error ?? "The project could not be created.",
    );
    nameRef.current?.focus();
  };

  return (
    <ModalDialog
      className="create-project-dialog dialog--scrim"
      labelledBy={titleId}
      describedBy={error ? errorId : undefined}
      initialFocusRef={nameRef}
      restoreFocus
      onDismiss={() => {
        if (!saving) onClose();
      }}
    >
      <form className="create-project-dialog__form" onSubmit={submit}>
        <header className="create-project-dialog__header">
          <h2 id={titleId}>Create project</h2>
          <button
            type="button"
            className="create-project-dialog__close"
            aria-label="Close"
            title="Close"
            disabled={saving}
            onClick={onClose}
          >
            <X size={16} aria-hidden="true" />
          </button>
        </header>
        <label className="create-project-dialog__name">
          <span className="create-project-dialog__name-icon" aria-hidden="true">
            <Folder size={16} />
          </span>
          <input
            ref={nameRef}
            aria-label="Project name"
            placeholder="Project name"
            maxLength={MAX_PROJECT_NAME_CHARS}
            disabled={saving}
            value={name}
            onChange={(event) => setName(event.currentTarget.value)}
          />
        </label>
        <section
          className="create-project-dialog__sources"
          aria-labelledby={`${titleId}-sources`}
        >
          <div className="create-project-dialog__sources-header">
            <h3 id={`${titleId}-sources`}>Source folders</h3>
            {folders.length > 0 ? (
              <span className="create-project-dialog__location">
                <Laptop size={14} aria-hidden="true" />
                This computer
              </span>
            ) : null}
          </div>
          {folders.length > 0 ? (
            <ul className="create-project-dialog__folders">
              {folders.map((folder) => (
                <li key={folder.token} title={folder.path}>
                  <Folder size={16} aria-hidden="true" />
                  <span className="create-project-dialog__folder-name">
                    {folder.name}
                  </span>
                  <button
                    type="button"
                    className="create-project-dialog__remove"
                    aria-label={`Remove ${folder.name}`}
                    title={`Remove ${folder.name}`}
                    disabled={saving}
                    onClick={() =>
                      setFolders((held) =>
                        held.filter((entry) => entry.token !== folder.token),
                      )
                    }
                  >
                    <X size={14} aria-hidden="true" />
                  </button>
                </li>
              ))}
              <li>
                <button
                  type="button"
                  className="create-project-dialog__add-row"
                  disabled={busy || full}
                  title={full ? fullMessage : undefined}
                  onClick={() => void addFolder()}
                >
                  <FolderPlus size={16} aria-hidden="true" />
                  Add folder
                </button>
              </li>
            </ul>
          ) : (
            <div className="create-project-dialog__source-empty">
              <p>
                Add a folder on <strong>this computer</strong>
              </p>
              <button
                type="button"
                className="create-project-dialog__add"
                disabled={busy}
                onClick={() => void addFolder()}
              >
                <FolderPlus size={14} aria-hidden="true" />
                Add
              </button>
            </div>
          )}
        </section>
        {error ? (
          <p className="create-project-dialog__error" id={errorId} role="alert">
            {error}
          </p>
        ) : null}
        <footer className="create-project-dialog__actions">
          <button
            type="button"
            className="create-project-dialog__cancel"
            disabled={saving}
            onClick={onClose}
          >
            Cancel
          </button>
          <button
            type="submit"
            className="create-project-dialog__submit"
            disabled={!projectName || busy}
          >
            Create project
          </button>
        </footer>
      </form>
    </ModalDialog>
  );
}

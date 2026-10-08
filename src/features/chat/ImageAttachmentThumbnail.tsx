import { useEffect, useState } from "react";

import { getChatImageThumbnail } from "../../services/imageArtifactDesktop";

export interface ImageThumbnailOwner {
  projectId: string;
  projectGeneration: string;
  chatId: string;
}

export function ImageAttachmentThumbnail({
  owner,
  artifactId,
  label,
}: {
  owner: ImageThumbnailOwner;
  artifactId: string;
  label: string;
}) {
  const { projectId, projectGeneration, chatId } = owner;
  const identity = `${projectId}\0${projectGeneration}\0${chatId}\0${artifactId}`;
  const [result, setResult] = useState<{
    identity: string;
    preview: string | null | "unavailable";
  }>();
  const preview = result?.identity === identity ? result.preview : undefined;
  useEffect(() => {
    let active = true;
    void getChatImageThumbnail({
      projectId,
      projectGeneration,
      chatId,
      artifactId,
    })
      .then((url) => {
        if (active) setResult({ identity, preview: url });
      })
      .catch(() => {
        if (active) setResult({ identity, preview: "unavailable" });
      });
    return () => {
      active = false;
    };
  }, [identity, projectId, projectGeneration, chatId, artifactId]);
  if (preview === undefined)
    return <span className="composer__image-placeholder">Loading image…</span>;
  if (preview === null)
    return (
      <span className="composer__image-placeholder">
        Image no longer stored
      </span>
    );
  if (preview === "unavailable")
    return (
      <span className="composer__image-placeholder">Image unavailable</span>
    );
  return (
    <img
      className="composer__image-thumbnail"
      src={preview}
      alt={`${label} thumbnail`}
    />
  );
}

import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type RefObject,
} from "react";

export const FOLLOW_END_THRESHOLD_PX = 64;

function followEndScrollBehavior(): ScrollBehavior {
  return window.matchMedia?.("(prefers-reduced-motion: reduce)").matches
    ? "auto"
    : "smooth";
}

function transcriptIsAtEnd(
  transcript: Pick<HTMLElement, "clientHeight" | "scrollHeight" | "scrollTop">,
): boolean {
  const distance =
    transcript.scrollHeight - transcript.scrollTop - transcript.clientHeight;
  return distance <= FOLLOW_END_THRESHOLD_PX;
}

export function useFollowEnd({
  ownerKey,
  revision,
  transcriptRef,
  contentRef,
  endRef,
}: {
  ownerKey: string;
  revision: string | number | undefined;
  transcriptRef: RefObject<HTMLElement | null>;
  contentRef: RefObject<HTMLElement | null>;
  endRef: RefObject<HTMLElement | null>;
}) {
  const [followingEnd, setFollowingEnd] = useState(true);
  const frameRef = useRef<number | null>(null);
  const followingEndRef = useRef(followingEnd);

  useEffect(() => {
    followingEndRef.current = followingEnd;
  }, [followingEnd]);

  const cancelFrame = useCallback(() => {
    if (frameRef.current !== null) cancelAnimationFrame(frameRef.current);
    frameRef.current = null;
  }, []);

  useEffect(() => cancelFrame, [cancelFrame, ownerKey]);

  const anchorToEnd = useCallback(
    (behavior: ScrollBehavior) => {
      const transcript = transcriptRef.current;
      if (!transcript) {
        endRef.current?.scrollIntoView({ behavior, block: "end" });
        return;
      }
      const top = transcript.scrollHeight - transcript.clientHeight;
      if (typeof transcript.scrollTo === "function") {
        transcript.scrollTo({ behavior, top });
        return;
      }
      transcript.scrollTop = top;
    },
    [endRef, transcriptRef],
  );

  useEffect(() => {
    if (!followingEnd) return;
    anchorToEnd(followEndScrollBehavior());
  }, [anchorToEnd, followingEnd, revision]);

  useEffect(() => {
    if (typeof ResizeObserver === "undefined") return;
    const transcript = transcriptRef.current;
    if (!transcript) return;

    const observer = new ResizeObserver(() => {
      const current = transcriptRef.current;
      if (!current || !followingEndRef.current) return;
      if (transcriptIsAtEnd(current)) return;
      anchorToEnd("auto");
    });
    observer.observe(transcript);
    const content = contentRef.current;
    if (content) observer.observe(content);

    return () => observer.disconnect();
  }, [anchorToEnd, contentRef, ownerKey, transcriptRef]);

  const updateFollowEnd = useCallback(() => {
    if (frameRef.current !== null) return;
    frameRef.current = requestAnimationFrame(() => {
      frameRef.current = null;
      const transcript = transcriptRef.current;
      if (transcript) setFollowingEnd(transcriptIsAtEnd(transcript));
    });
  }, [transcriptRef]);

  const jumpToLatest = useCallback(() => {
    cancelFrame();
    setFollowingEnd(true);
    anchorToEnd(followEndScrollBehavior());
  }, [anchorToEnd, cancelFrame]);

  return { followingEnd, updateFollowEnd, jumpToLatest };
}

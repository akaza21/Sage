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
  return distanceFromEnd(transcript) <= FOLLOW_END_THRESHOLD_PX;
}

function distanceFromEnd(
  transcript: Pick<HTMLElement, "clientHeight" | "scrollHeight" | "scrollTop">,
): number {
  return (
    transcript.scrollHeight - transcript.scrollTop - transcript.clientHeight
  );
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
  const [followState, setFollowState] = useState({ ownerKey, following: true });
  const followingEnd =
    followState.ownerKey === ownerKey ? followState.following : true;
  const [jumpState, setJumpState] = useState({ ownerKey, visible: false });
  const showJumpToLatest = jumpState.ownerKey === ownerKey && jumpState.visible;
  const frameRef = useRef<number | null>(null);
  const jumpingRef = useRef(false);
  // Own anchor steps keep following until arrival, reader input, backward
  // movement, or owner change, even when resizing makes their target stale.
  const anchoringRef = useRef(false);
  const lastScrollTopRef = useRef(0);
  const followingEndRef = useRef(followingEnd);

  useEffect(() => {
    followingEndRef.current = followingEnd;
  }, [followingEnd]);

  const cancelFrame = useCallback(() => {
    if (frameRef.current !== null) cancelAnimationFrame(frameRef.current);
    frameRef.current = null;
  }, []);

  useEffect(() => {
    jumpingRef.current = false;
    anchoringRef.current = false;
    lastScrollTopRef.current = transcriptRef.current?.scrollTop ?? 0;
    return cancelFrame;
  }, [cancelFrame, ownerKey, transcriptRef]);

  const anchorToEnd = useCallback(
    (behavior: ScrollBehavior) => {
      const transcript = transcriptRef.current;
      if (!transcript) {
        endRef.current?.scrollIntoView({ behavior, block: "end" });
        return;
      }
      const top = transcript.scrollHeight - transcript.clientHeight;
      if (Math.abs(top - transcript.scrollTop) > 1) anchoringRef.current = true;
      if (typeof transcript.scrollTo === "function") {
        transcript.scrollTo({ behavior, top });
        return;
      }
      transcript.scrollTop = top;
    },
    [endRef, transcriptRef],
  );

  const interruptJump = useCallback(() => {
    if (!jumpingRef.current) return;
    jumpingRef.current = false;
    followingEndRef.current = false;
    setFollowState({ ownerKey, following: false });
    const transcript = transcriptRef.current;
    if (transcript) {
      transcript.scrollTo({ behavior: "auto", top: transcript.scrollTop });
      setJumpState({
        ownerKey,
        visible: distanceFromEnd(transcript) > transcript.clientHeight,
      });
    }
  }, [ownerKey, transcriptRef]);

  useEffect(() => {
    const transcript = transcriptRef.current;
    if (!transcript) return;
    const readerInput = () => {
      anchoringRef.current = false;
      interruptJump();
    };
    // Any pointerdown inside the transcript counts, including child controls.
    transcript.addEventListener("wheel", readerInput);
    transcript.addEventListener("pointerdown", readerInput);
    transcript.addEventListener("touchstart", readerInput);
    transcript.addEventListener("keydown", readerInput);
    return () => {
      transcript.removeEventListener("wheel", readerInput);
      transcript.removeEventListener("pointerdown", readerInput);
      transcript.removeEventListener("touchstart", readerInput);
      transcript.removeEventListener("keydown", readerInput);
    };
  }, [interruptJump, ownerKey, transcriptRef]);

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
      if (!current) return;
      if (jumpingRef.current) return;
      if (!followingEndRef.current) {
        setJumpState({
          ownerKey,
          visible: distanceFromEnd(current) > current.clientHeight,
        });
        return;
      }
      // Keep stale smooth steps protected until the anchor actually lands.
      if (distanceFromEnd(current) <= 1) anchoringRef.current = false;
      if (transcriptIsAtEnd(current)) return;
      anchorToEnd("auto");
    });
    observer.observe(transcript);
    const content = contentRef.current;
    if (content) observer.observe(content);

    return () => observer.disconnect();
  }, [anchorToEnd, contentRef, ownerKey, transcriptRef]);

  const updateFollowEnd = useCallback(() => {
    const current = transcriptRef.current;
    if (current) {
      // Backward movement is reader-owned. Forward find-in-page/scrollIntoView
      // jumps are indistinguishable from own steps while an anchor travels.
      if (current.scrollTop < lastScrollTopRef.current - 1)
        anchoringRef.current = false;
      lastScrollTopRef.current = current.scrollTop;
      if (
        !jumpingRef.current &&
        !anchoringRef.current &&
        !transcriptIsAtEnd(current)
      )
        followingEndRef.current = false;
    }
    if (frameRef.current !== null) return;
    frameRef.current = requestAnimationFrame(() => {
      frameRef.current = null;
      const transcript = transcriptRef.current;
      if (transcript) {
        if (jumpingRef.current || anchoringRef.current) {
          if (transcriptIsAtEnd(transcript)) jumpingRef.current = false;
          if (distanceFromEnd(transcript) <= 1) anchoringRef.current = false;
          return;
        }
        setFollowState({
          ownerKey,
          following: transcriptIsAtEnd(transcript),
        });
        setJumpState({
          ownerKey,
          visible: distanceFromEnd(transcript) > transcript.clientHeight,
        });
      }
    });
  }, [ownerKey, transcriptRef]);

  const jumpToLatest = useCallback(() => {
    cancelFrame();
    jumpingRef.current = true;
    setFollowState({ ownerKey, following: true });
    setJumpState({ ownerKey, visible: false });
    anchorToEnd(followEndScrollBehavior());
  }, [anchorToEnd, cancelFrame, ownerKey]);

  return { followingEnd, showJumpToLatest, updateFollowEnd, jumpToLatest };
}

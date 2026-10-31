import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type RefObject,
} from "react";

export const FOLLOW_END_THRESHOLD_PX = 64;

export const JUMP_DURATION_MS = 320;

function followEndScrollBehavior(): ScrollBehavior {
  return window.matchMedia?.("(prefers-reduced-motion: reduce)").matches
    ? "auto"
    : "smooth";
}

function scrollTranscript(
  transcript: HTMLElement,
  top: number,
  behavior: ScrollBehavior,
) {
  if (typeof transcript.scrollTo === "function")
    transcript.scrollTo({ behavior, top });
  else transcript.scrollTop = top;
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
  running = false,
  transcriptRef,
  contentRef,
  endRef,
}: {
  ownerKey: string;
  revision: string | number | undefined;
  /** A live turn: growth below a paused reader shows the activity pill. */
  running?: boolean;
  transcriptRef: RefObject<HTMLElement | null>;
  contentRef: RefObject<HTMLElement | null>;
  endRef: RefObject<HTMLElement | null>;
}) {
  const [followState, setFollowState] = useState({ ownerKey, following: true });
  const followingEnd =
    followState.ownerKey === ownerKey ? followState.following : true;
  // "activity": new rows landed below a paused reader.
  const [jumpState, setJumpState] = useState<{
    ownerKey: string;
    visible: boolean | "activity";
  }>({ ownerKey, visible: false });
  const showJumpToLatest = jumpState.ownerKey === ownerKey && jumpState.visible;
  const frameRef = useRef<number | null>(null);
  const jumpingRef = useRef(false);
  // Own anchor steps keep following until arrival, reader input, backward
  // movement, or owner change, even when resizing makes their target stale.
  const anchoringRef = useRef(false);
  const lastScrollTopRef = useRef(0);
  const followingEndRef = useRef(followingEnd);
  const navigationPausedRef = useRef(false);
  // Content height while the reader last followed the end.
  const followedHeightRef = useRef(Infinity);
  const jumpFor = useCallback(
    (transcript: HTMLElement) => ({
      ownerKey,
      visible:
        running && transcript.scrollHeight > followedHeightRef.current
          ? ("activity" as const)
          : distanceFromEnd(transcript) > transcript.clientHeight,
    }),
    [ownerKey, running],
  );

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
    navigationPausedRef.current = false;
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
      scrollTranscript(transcript, top, behavior);
    },
    [endRef, transcriptRef],
  );

  const interruptJump = useCallback(() => {
    if (!jumpingRef.current) return;
    cancelFrame();
    jumpingRef.current = false;
    followingEndRef.current = false;
    setFollowState({ ownerKey, following: false });
    const transcript = transcriptRef.current;
    if (transcript) {
      transcript.scrollTo({ behavior: "auto", top: transcript.scrollTop });
      setJumpState(jumpFor(transcript));
    }
  }, [cancelFrame, ownerKey, jumpFor, transcriptRef]);

  useEffect(() => {
    const transcript = transcriptRef.current;
    if (!transcript) return;
    const readerInput = () => {
      navigationPausedRef.current = false;
      anchoringRef.current = false;
      interruptJump();
    };
    const navigate = () => {
      readerInput();
      navigationPausedRef.current = true;
      followingEndRef.current = false;
      setFollowState({ ownerKey, following: false });
    };
    // An upward wheel that will move the transcript leaves the end at once:
    // its smooth scroll can still be inside the follow threshold when new
    // activity lands, and anchoring then would pull the reader back down
    //. A short chat, or a nested scroller that
    // takes the wheel itself, keeps following.
    const movesTranscriptUp = (event: WheelEvent) => {
      // A synthetic wheel without a deltaY is reader input, not navigation.
      if (!(event.deltaY < 0 && transcript.scrollTop > 0)) return false;
      for (
        let node = event.target instanceof Element ? event.target : null;
        node && node !== transcript;
        node = node.parentElement
      )
        if (node.scrollTop > 0) return false;
      return true;
    };
    const wheel = (event: WheelEvent) =>
      movesTranscriptUp(event) ? navigate() : readerInput();
    // Any pointerdown inside the transcript counts, including child controls.
    transcript.addEventListener("wheel", wheel);
    transcript.addEventListener("pointerdown", readerInput);
    transcript.addEventListener("touchstart", readerInput);
    transcript.addEventListener("keydown", readerInput);
    transcript.addEventListener("sage-chat-navigation", navigate);
    return () => {
      transcript.removeEventListener("wheel", wheel);
      transcript.removeEventListener("pointerdown", readerInput);
      transcript.removeEventListener("touchstart", readerInput);
      transcript.removeEventListener("keydown", readerInput);
      transcript.removeEventListener("sage-chat-navigation", navigate);
    };
  }, [interruptJump, ownerKey, transcriptRef]);

  useEffect(() => {
    // A jump drives its own travel; a native smooth scroll would fight it.
    if (!followingEnd || jumpingRef.current) return;
    anchorToEnd(followEndScrollBehavior());
  }, [anchorToEnd, followingEnd, revision]);

  useEffect(() => {
    if (typeof ResizeObserver === "undefined") return;
    const transcript = transcriptRef.current;
    if (!transcript) return;

    const observer = new ResizeObserver(() => {
      const current = transcriptRef.current;
      if (!current) return;
      // A travelling jump retargets the moving end on its next frame.
      if (jumpingRef.current) return;
      if (!followingEndRef.current) {
        setJumpState(jumpFor(current));
        return;
      }
      followedHeightRef.current = current.scrollHeight;
      // Keep stale smooth steps protected until the anchor actually lands.
      if (distanceFromEnd(current) <= 1) {
        anchoringRef.current = false;
        return;
      }
      anchorToEnd("auto");
    });
    observer.observe(transcript);
    const content = contentRef.current;
    if (content) observer.observe(content);

    return () => observer.disconnect();
  }, [anchorToEnd, contentRef, jumpFor, transcriptRef]);

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
        (!transcriptIsAtEnd(current) || navigationPausedRef.current)
      )
        followingEndRef.current = false;
    }
    if (frameRef.current !== null) return;
    frameRef.current = requestAnimationFrame(() => {
      frameRef.current = null;
      const transcript = transcriptRef.current;
      if (transcript) {
        const atEnd = transcriptIsAtEnd(transcript);
        // Nothing is below the fold at the end, including where a jump lands.
        if (atEnd) followedHeightRef.current = transcript.scrollHeight;
        if (jumpingRef.current || anchoringRef.current) {
          if (atEnd) jumpingRef.current = false;
          if (distanceFromEnd(transcript) <= 1) anchoringRef.current = false;
          return;
        }
        setFollowState({
          ownerKey,
          following: !navigationPausedRef.current && atEnd,
        });
        setJumpState(jumpFor(transcript));
      }
    });
  }, [ownerKey, jumpFor, transcriptRef]);

  const jumpToLatest = useCallback(() => {
    navigationPausedRef.current = false;
    cancelFrame();
    jumpingRef.current = true;
    setFollowState({ ownerKey, following: true });
    const hide = () => setJumpState({ ownerKey, visible: false });
    const transcript = transcriptRef.current;
    if (!transcript || followEndScrollBehavior() === "auto") {
      hide();
      anchorToEnd("auto");
      jumpingRef.current = false;
      return;
    }
    // A timed ease-out tween rather than a native smooth scroll, which
    // WebView2 can hold for frames; each frame retargets the moving end.
    const from = transcript.scrollTop;
    let started: number | null = null;
    const step = (now: number) => {
      frameRef.current = null;
      const current = transcriptRef.current;
      if (!jumpingRef.current || !current) return;
      started ??= now;
      const progress = Math.min(1, (now - started) / JUMP_DURATION_MS);
      const end = current.scrollHeight - current.clientHeight;
      scrollTranscript(
        current,
        from + (end - from) * (1 - (1 - progress) ** 3),
        "auto",
      );
      if (progress < 1) {
        frameRef.current = requestAnimationFrame(step);
        return;
      }
      jumpingRef.current = false;
      anchoringRef.current = distanceFromEnd(current) > 1;
      // The pill stays through the travel and fades once it lands (s6/f0699).
      hide();
    };
    frameRef.current = requestAnimationFrame(step);
  }, [anchorToEnd, cancelFrame, ownerKey, transcriptRef]);

  return { followingEnd, showJumpToLatest, updateFollowEnd, jumpToLatest };
}

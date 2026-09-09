import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type RefObject,
} from "react";

import { followEndScrollBehavior, transcriptIsAtEnd } from "./chatFollowEnd";

export function useFollowEnd({
  ownerKey,
  revision,
  transcriptRef,
  endRef,
}: {
  ownerKey: string;
  revision: string | number | undefined;
  transcriptRef: RefObject<HTMLElement | null>;
  endRef: RefObject<HTMLElement | null>;
}) {
  const [followingEnd, setFollowingEnd] = useState(true);
  const frameRef = useRef<number | null>(null);

  const cancelFrame = useCallback(() => {
    if (frameRef.current !== null) cancelAnimationFrame(frameRef.current);
    frameRef.current = null;
  }, []);

  useEffect(() => cancelFrame, [cancelFrame, ownerKey]);

  useEffect(() => {
    if (!followingEnd) return;
    endRef.current?.scrollIntoView({
      behavior: followEndScrollBehavior(),
      block: "end",
    });
  }, [endRef, followingEnd, revision]);

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
    endRef.current?.scrollIntoView({
      behavior: followEndScrollBehavior(),
      block: "end",
    });
  }, [cancelFrame, endRef]);

  return { followingEnd, updateFollowEnd, jumpToLatest };
}

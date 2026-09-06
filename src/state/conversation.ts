import { useCallback, useRef, useState } from "react";
import { useTauriEvent } from "../platform/events";
import { channelEventNames } from "../platform/eventNames";
import type { ChannelId, TranscriptEvent } from "../platform/types";

export interface ConversationTurn {
  id: string;
  /** "me" is the owner's own microphone; "them" is the other person, off
   *  the computer's speaker/system-audio channel. */
  speaker: "me" | "them";
  text: string;
  /** False while this turn is still being spoken (the words can still
   *  change); true once the speech engine has settled on the final text. */
  final: boolean;
}

export interface ConversationFeedState {
  turns: ConversationTurn[];
  error: string | null;
  /** Starts a fresh feed, e.g. when a new session begins. */
  clear: () => void;
}

const CHANNEL_SPEAKER: Record<ChannelId, ConversationTurn["speaker"]> = {
  mic: "me",
  system: "them",
};

function newId(): string {
  return typeof crypto.randomUUID === "function"
    ? crypto.randomUUID()
    : `turn-${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

/**
 * The live conversation as it is spoken, one bubble per turn, clearly
 * labelled "me" versus "them". Each channel's in-progress words update
 * the same bubble live; once the speech engine settles on final text that
 * bubble is marked final and the next thing said on that channel starts a
 * new one. This is a display-only feed — the AI's memory of the
 * conversation is a separate thing (`useWindowMemory`), built only from
 * finals, never from what's shown here mid-sentence.
 */
export function useConversationFeed(): ConversationFeedState {
  const [turns, setTurns] = useState<ConversationTurn[]>([]);
  const [error, setError] = useState<string | null>(null);
  const pendingId = useRef<Record<ChannelId, string | null>>({ mic: null, system: null });

  const applyEvent = useCallback((channel: ChannelId, payload: TranscriptEvent) => {
    const speaker = CHANNEL_SPEAKER[channel];
    const text = payload.text;
    if (text.trim().length === 0 && !payload.is_final) return;

    setTurns((prev) => {
      const openId = pendingId.current[channel];
      if (openId) {
        const index = prev.findIndex((turn) => turn.id === openId);
        if (index !== -1) {
          const next = prev.slice();
          next[index] = { ...next[index], text, final: payload.is_final };
          if (payload.is_final) pendingId.current[channel] = null;
          return next;
        }
      }
      const id = newId();
      if (!payload.is_final) pendingId.current[channel] = id;
      return [...prev, { id, speaker, text, final: payload.is_final }];
    });
  }, []);

  const micEvents = channelEventNames("mic");
  const systemEvents = channelEventNames("system");

  useTauriEvent<TranscriptEvent>(micEvents.transcript, (payload) => applyEvent("mic", payload));
  useTauriEvent<TranscriptEvent>(systemEvents.transcript, (payload) => applyEvent("system", payload));
  useTauriEvent<string>(micEvents.transcriptError, (payload) => setError(payload));
  useTauriEvent<string>(systemEvents.transcriptError, (payload) => setError(payload));

  const clear = useCallback(() => {
    pendingId.current = { mic: null, system: null };
    setTurns([]);
    setError(null);
  }, []);

  return { turns, error, clear };
}

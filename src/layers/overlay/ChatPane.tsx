import { useState } from "react";
import type { LiveTranscriptTurn } from "../../platform/types";

export interface ChatPaneProps {
  disabled: boolean;
  streaming: boolean;
  hasAnyMessage: boolean;
  turns: LiveTranscriptTurn[];
  onSend: (message: string) => void;
}

/**
 * Spec §7 — replaces the answer card body while chat is open. Sending
 * reuses the exact same typed-question flow `AskBox.tsx` (live-answer)
 * already calls (`useAnswerStream().ask`); this component only owns the
 * composer's local text, not the answer stream itself.
 */
export function ChatPane({ disabled, streaming, hasAnyMessage, turns, onSend }: ChatPaneProps) {
  const [value, setValue] = useState("");

  const submit = () => {
    const trimmed = value.trim();
    if (trimmed.length === 0 || streaming) return;
    onSend(trimmed);
    setValue("");
  };

  return (
    <div>
      {!hasAnyMessage && turns.length === 0 ? <p className="ov-chat-empty">Conversation will appear here as people speak.</p> : null}
      {turns.length > 0 ? <div className="ov-chat-turns" aria-label="Live conversation">
        {turns.map((turn, index) => <p className={`ov-chat-turn ov-chat-turn--${turn.channel}`} key={`${turn.channel}-${index}`}><b>{turn.channel === "mic" ? "You" : "Others"}</b><span>{turn.text}</span></p>)}
      </div> : null}
      <div className="ov-chat-composer">
        <input
          type="text"
          placeholder="Type a manual message…"
          value={value}
          disabled={disabled}
          onChange={(event) => setValue(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
              event.preventDefault();
              submit();
            }
          }}
        />
        <button type="button" className="ov-pill" disabled={disabled || streaming || value.trim().length === 0} onClick={submit}>
          {streaming ? "Sending…" : "Send"}
        </button>
      </div>
    </div>
  );
}

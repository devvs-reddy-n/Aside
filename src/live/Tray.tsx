import { useEffect, useRef, useState, type FormEvent } from "react";
import { Icon } from "../kit/Icon";
import { useController, useLive, useLiveUi } from "./context";
import type { AnswerLength } from "../bridge/api";
import type { Room } from "./hooks";
import { isAnswering } from "./machine";

const LENGTHS: Array<[AnswerLength, string, string]> = [
  ["small", "Short", "Generate a short answer"],
  ["medium", "Medium", "Generate a medium-length answer"],
  ["long", "Long", "Generate a detailed answer"],
];

/** The bottom edge: generate an automatic or fixed-length answer, take a screenshot, or type a follow-up. It never scrolls away. */
export function Footer({ room }: { room: Room }) {
  const controller = useController();
  const answering = useLive(isAnswering);
  const shotBusy = useLiveUi((state) => state.screenshotBusy);
  const [askOpen, setAskOpen] = useState(false);
  const askButton = useRef<HTMLButtonElement>(null);
  const [text, setText] = useState("");
  const input = useRef<HTMLInputElement>(null);
  const short = room.height !== "regular";

  useEffect(() => { if (askOpen) input.current?.focus(); }, [askOpen]);

  async function submit(event: FormEvent) {
    event.preventDefault();
    const question = text.trim();
    if (!question || answering) return;
    const ok = await controller.ask(question);
    if (ok) setText((current) => current.trim() === question ? "" : current);
  }

  const answerButton = (
    <button type="button" className="lv-btn lv-answer-btn" data-primary disabled={answering} aria-busy={answering || undefined} aria-label="Answer automatically" onClick={() => void controller.answer()} title="Answer — length adapts to the question  ⌘↩">
      {answering ? <span className="lv-spin" aria-hidden="true" /> : <Icon name="play" size={24} />}
    </button>
  );
  const shotButton = (
    <button type="button" className="lv-icon lv-icon--boxed" disabled={answering || shotBusy} onClick={() => void controller.screenshot()} aria-label="Screenshot answer" title="Screenshot answer  ⌘⇧↩">
      {shotBusy ? <span className="lv-spin" aria-hidden="true" /> : <Icon name="camera" size={17} />}
    </button>
  );
  const composer = (
    <form className="lv-ask" id="live-follow-up" onSubmit={(event) => void submit(event)} onKeyDown={(event) => { if (event.key === "Escape") { event.stopPropagation(); setAskOpen(false); askButton.current?.focus(); } }}>
      <input
        ref={input}
        value={text}
        onChange={(event) => setText(event.target.value)}
        placeholder="Ask a follow-up…"
        aria-label="Ask a follow-up question"
        autoComplete="off"
        spellCheck
      />
      <button type="submit" className="lv-icon lv-icon--send" disabled={!text.trim() || answering} aria-label="Send question">
        <Icon name="send" size={16} />
      </button>
    </form>
  );

  return (
    <div className="lv-tray">
      <footer className="lv-foot" data-compact={short || undefined}>
        {askOpen ? composer : null}
        <div className="lv-foot__row lv-foot__answers">
          <div className="lv-length-actions" role="group" aria-label="Answer at a specific length">
            {LENGTHS.map(([value, label, title]) => (
              <button key={value} type="button" className="lv-btn lv-length-btn" disabled={answering} title={title} onClick={() => void controller.answer(value)}>{label}</button>
            ))}
          </div>
          <button ref={askButton} type="button" className="lv-icon lv-icon--boxed" aria-label={askOpen ? "Close question input" : "Ask a question"} title="Ask a question" aria-expanded={askOpen} aria-controls="live-follow-up" onClick={() => setAskOpen((open) => !open)}><Icon name={askOpen ? "x" : "chat"} size={16} /></button>
          {shotButton}
          {answerButton}
        </div>
      </footer>
    </div>
  );
}

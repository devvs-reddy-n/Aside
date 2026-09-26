import { useEffect, useRef, useState, type FormEvent } from "react";
import { Icon } from "../kit/Icon";
import { useController, useLive, useLiveUi } from "./context";
import type { Room } from "./hooks";
import { isAnswering } from "./machine";

function QuestionCard() {
  const controller = useController();
  const question = useLive((state) => state.question);
  const answering = useLive(isAnswering);
  if (!question || answering) return null;
  return (
    <section className="lv-question" aria-label="Question detected" key={question.turnId + question.text.length}>
      <p className="lv-question__tag"><Icon name="spark" size={14} />Question detected</p>
      <blockquote className="selectable">{question.text}</blockquote>
      <div className="lv-question__actions">
        <button type="button" className="lv-btn" data-primary onClick={() => void controller.answer()}>Answer this<kbd>⌘↩</kbd></button>
        <button type="button" className="lv-btn" data-quiet onClick={() => controller.dispatch({ type: "dismiss-question" })}>Not now</button>
      </div>
    </section>
  );
}

const LENGTHS: Array<["auto" | "small" | "medium" | "long", string, string]> = [
  ["small", "S", "Short answer"],
  ["medium", "M", "Medium answer"],
  ["long", "L", "Long answer"],
];

/** The bottom edge: ask for an answer, pick its length, take a screenshot, or type a follow-up. It never scrolls away. */
export function Footer({ room }: { room: Room }) {
  const controller = useController();
  const answering = useLive(isAnswering);
  const shotBusy = useLiveUi((state) => state.screenshotBusy);
  const length = useLiveUi((state) => state.length);
  const askTick = useLiveUi((state) => state.askTick);
  /** While a detected question is on screen, its "Answer this" is the one blue button; this one steps back. */
  const questionShowing = useLive((state) => state.question !== null) && !answering;
  const [text, setText] = useState("");
  const input = useRef<HTMLInputElement>(null);
  const short = room.height !== "regular";

  useEffect(() => {
    if (askTick > 0) input.current?.focus();
  }, [askTick]);

  async function submit(event: FormEvent) {
    event.preventDefault();
    const question = text.trim();
    if (!question || answering) return;
    const ok = await controller.ask(question);
    if (ok) setText("");
  }

  const answerButton = (
    <button type="button" className="lv-btn lv-answer-btn" data-primary={questionShowing ? undefined : ""} data-quiet={questionShowing ? "" : undefined} disabled={answering} onClick={() => void controller.answer()} title="Answer the latest question (⌘↩)">
      {answering ? <span className="lv-spin" aria-hidden="true" /> : <Icon name="play" size={13} />}
      <span>{answering ? "Answering…" : "Answer"}</span>
      {answering ? null : <kbd>⌘↩</kbd>}
    </button>
  );
  const shotButton = (
    <button type="button" className="lv-icon lv-icon--boxed" disabled={answering || shotBusy} onClick={() => void controller.screenshot()} aria-label="Answer from a screenshot" title="Answer from a screenshot (⌘K)">
      {shotBusy ? <span className="lv-spin" aria-hidden="true" /> : <Icon name="camera" size={17} />}
    </button>
  );
  const composer = (
    <form className="lv-ask" onSubmit={(event) => void submit(event)}>
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
      <QuestionCard />
      <footer className="lv-foot" data-compact={short || undefined}>
        {short ? (
          <div className="lv-foot__row">{answerButton}{shotButton}{composer}</div>
        ) : (
          <>
            <div className="lv-foot__row">
              {answerButton}
              <div className="lv-seg lv-seg--len" role="group" aria-label="Answer length">
                {LENGTHS.map(([value, label, title]) => (
                  <button key={value} type="button" title={title} aria-pressed={length === value} onClick={() => controller.setLength(length === value ? "auto" : value)}>{label}</button>
                ))}
              </div>
              {shotButton}
            </div>
            {composer}
          </>
        )}
      </footer>
    </div>
  );
}

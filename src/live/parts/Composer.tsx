import { ANSWER_SHORTCUT, SCREENSHOT_SHORTCUT } from "../../core/shortcuts";
import { useRef, useState, type FormEvent } from "react";
import { Icon } from "../../kit/Icon";
import { AnswerHistory } from "./History";
import { useController, useLive, useLiveUi } from "../context";
import { isAnswering, type AnswerLengthChoice } from "../machine";

const LENGTHS = [["small", "Short"], ["medium", "Med"], ["long", "Long"]] as const;

/** One shared draft for question-row utilities and floating answer actions. */
export function useComposer() {
  const controller = useController();
  const answering = useLive(isAnswering);
  const shooting = useLiveUi((state) => state.screenshotBusy);
  const [text, setText] = useState("");
  const [expanded, setExpanded] = useState(false);
  const search = useRef<HTMLButtonElement>(null);
  const busy = answering || shooting;

  function closeSearch() {
    setExpanded(false);
    search.current?.focus();
  }

  async function answer(length: AnswerLengthChoice = "auto") {
    if (busy) return;
    const question = text.trim();
    if (expanded && question) {
      const ok = await controller.ask(question, length);
      if (ok) {
        setText((current) => current.trim() === question ? "" : current);
        closeSearch();
      }
    } else {
      await controller.answer(length);
    }
  }

  function submit(event: FormEvent) {
    event.preventDefault();
    if (text.trim()) void answer();
  }

  return { controller, answering, shooting, busy, text, setText, expanded, setExpanded, search, closeSearch, answer, submit };
}

type ComposerState = ReturnType<typeof useComposer>;

/** Search and screenshot stay beside the question, even before speech arrives. */
export function QuestionActions({ state }: { state: ComposerState }) {
  const { controller, shooting, busy, expanded, setExpanded, search } = state;
  return (
    <div className="lv-question-actions" aria-label="Question controls">
      <button ref={search} type="button" className="lv-iconbtn lv-iconbtn--field" aria-label={expanded ? "Close follow-up search" : "Open follow-up search"} aria-expanded={expanded} aria-controls={expanded ? "lv-follow-up" : undefined} title="Ask a follow-up" onClick={() => setExpanded((open) => !open)}>
        <Icon name={expanded ? "x" : "search"} size={16} />
      </button>
      <button type="button" className="lv-iconbtn lv-iconbtn--field" disabled={busy} onClick={() => void controller.screenshot()} aria-label="Answer from a screenshot" title={`Answer from a screenshot  ${SCREENSHOT_SHORTCUT}`}>
        {shooting ? <span className="lv-spin" aria-hidden="true" /> : <Icon name="camera" size={16} />}
      </button>
    </div>
  );
}

/** The field expands below the whole question row, without covering the title. */
export function FollowUp({ state }: { state: ComposerState }) {
  const { expanded, text, setText, submit, closeSearch, busy } = state;
  return (
    <>
      {expanded ? (
        <form id="lv-follow-up" className="lv-ask" onSubmit={submit} onKeyDown={(event) => {
          if (event.key === "Escape") { event.stopPropagation(); closeSearch(); }
        }}>
          <input
            autoFocus
            value={text}
            onChange={(event) => setText(event.target.value)}
            placeholder="Ask a follow-up"
            aria-label="Ask a follow-up question"
            autoComplete="off"
            spellCheck
          />
          <button type="submit" className="lv-iconbtn" disabled={busy || !text.trim()} aria-label="Send question" title="Send  ↩">
            <Icon name="send" size={15} strokeWidth={2.25} />
          </button>
        </form>
      ) : null}
    </>
  );
}

/** Answer chooses length; the divided capsule overrides one answer. */
export function Composer({ state }: { state: ComposerState }) {
  const { busy, answering, answer } = state;
  return (
    <footer className="lv-composer" aria-label="Answer controls">
      <AnswerHistory />
      <span className="lv-grow" aria-hidden="true" />
      <div className="lv-lengths" role="group" aria-label="Answer length">
        {LENGTHS.map(([length, label]) => (
          <button key={length} type="button" disabled={busy} onClick={() => void answer(length)} aria-label={`${length === "medium" ? "Medium" : label} answer`} title={`Request a ${length === "medium" ? "medium" : label.toLowerCase()} answer`}>{label}</button>
        ))}
      </div>
      <button type="button" className="lv-go" disabled={busy} aria-busy={answering || undefined} aria-label="Answer now" title={`Answer · Auto length  ${ANSWER_SHORTCUT}`} onClick={() => void answer()}>
        {answering ? <span className="lv-spin" aria-hidden="true" /> : null}
        <span>Answer</span>
      </button>
    </footer>
  );
}

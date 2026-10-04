import { Icon } from "../../kit/Icon";
import { useController, useLive, useLiveUi } from "../context";
import type { AnswerPage } from "../navigation";
import { askedLine } from "./AnswerCard";
import { useAnswerPage } from "./status";

/** ‹ 2 / 5 › — step through earlier questions and answers (also ⌘< and ⌘>). */
export function AnswerNav({ compact = false }: { compact?: boolean }) {
  const controller = useController();
  const { index, count } = useAnswerPage();
  if (count < 2) return null;
  return (
    <nav className="lv-nav" data-compact={compact || undefined} aria-label="Answer history">
      <button type="button" className="lv-iconbtn lv-iconbtn--sm" aria-label="Previous answer" title="Previous answer  ⌘<" disabled={index <= 0} onClick={() => controller.stepAnswer(-1)}><Icon name="chevronLeft" size={16} /></button>
      <span className="tabular" aria-live="polite">{index + 1} / {count}</span>
      <button type="button" className="lv-iconbtn lv-iconbtn--sm" aria-label="Next answer" title="Next answer  ⌘>" disabled={index >= count - 1} onClick={() => controller.stepAnswer(1)}><Icon name="chevronRight" size={16} /></button>
    </nav>
  );
}

function whoAsked(page: AnswerPage): string {
  if (page.kind === "question") return "They asked";
  if (page.answer.source === "typed") return "You asked";
  if (page.answer.source === "screen") return "On your screen";
  return "They asked";
}

/** What is being answered, so the answer below always has its question right above it. */
export function QuestionCard({ page, oneLine = false }: { page: AnswerPage | undefined; oneLine?: boolean }) {
  const paused = useLiveUi((state) => state.paused !== null);
  const bothOff = useLive((state) => state.channels.mic.status === "off" && state.channels.system.status === "off");
  const text = page ? (page.kind === "question" ? page.question.text : askedLine(page.answer)) : null;

  if (!page || !text) {
    const resting = paused || bothOff;
    return (
      <section className="lv-question" data-empty="" data-one-line={oneLine || undefined} aria-label="Question">
        <span className="lv-question__icon" aria-hidden="true"><Icon name={resting ? "pause" : "wave"} size={15} strokeWidth={2} /></span>
        <div className="lv-question__main">
          <p className="lv-question__tag">{resting ? "Listening is paused" : "Listening for a question…"}</p>
          {oneLine ? null : <p className="lv-question__hint">{resting ? "Turn on You or Them to keep listening." : "When they ask something it shows up here. Press Answer any time."}</p>}
        </div>
      </section>
    );
  }

  return (
    <section className="lv-question" data-fresh={page.kind === "question" || undefined} data-one-line={oneLine || undefined} aria-label="Question">
      <span className="lv-question__icon" aria-hidden="true"><Icon name={page.kind === "answer" && page.answer.source === "screen" ? "screen" : "question"} size={15} strokeWidth={2} /></span>
      <div className="lv-question__main">
        <p className="lv-question__tag">{whoAsked(page)}</p>
        <p className="lv-question__text selectable" title={oneLine ? text : undefined}>{text}</p>
        {page.kind === "question" && !oneLine ? <p className="lv-question__hint">Press Answer, or pick a length below.</p> : null}
      </div>
    </section>
  );
}

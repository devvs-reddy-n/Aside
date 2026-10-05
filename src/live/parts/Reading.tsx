import { useLayoutEffect, useRef } from "react";
import { parseAnswerText } from "../../core/answerParse";
import { Icon } from "../../kit/Icon";
import { useController, useLive, useLiveUi } from "../context";
import { describeAnswerError } from "../errors";
import type { Answer } from "../machine";
import type { AnswerPage } from "../navigation";
import { Code } from "./Code";
import { useAnswerPage } from "./status";

/** While an answer streams, a code fence may not be closed yet. Close it so code renders as code the whole time. */
function forParsing(text: string, streaming: boolean): string {
  if (!streaming) return text;
  const fences = (text.match(/```/g) ?? []).length;
  return fences % 2 === 1 ? `${text}\n\`\`\`` : text;
}

/** What is being answered: the cleaned-up question if Aside made one, otherwise the words as heard. */
function questionOf(page: AnswerPage): string | null {
  const text = page.kind === "question" ? page.question.text : (page.answer.cleanedQuestion ?? page.answer.question);
  return text.trim() || null;
}

/** The answer itself: plain text, any list the AI wrote, and code. Only problems add a line underneath. */
function AnswerText({ answer }: { answer: Answer }) {
  const controller = useController();
  const streaming = answer.status === "streaming";
  const parsed = parseAnswerText(forParsing(answer.text, streaming));
  const friendly = answer.error ? describeAnswerError(answer.error) : null;

  return (
    <article className="lv-answer selectable" data-state={answer.status} aria-busy={streaming || undefined} aria-label="Answer">
      {streaming && !answer.text ? <p className="lv-think" role="status"><i /><i /><i /></p> : null}
      {parsed.prose ? (
        <p className="lv-prose">
          {parsed.prose}
          {streaming && !parsed.code ? <span className="lv-caret" aria-hidden="true" /> : null}
        </p>
      ) : null}
      {parsed.keySteps ? <ul className="lv-steps">{parsed.keySteps.map((step, index) => <li key={index}>{step}</li>)}</ul> : null}
      {parsed.code ? <Code block={parsed.code} streaming={streaming} /> : null}

      {answer.status === "error" && friendly ? (
        <div className="lv-problem">
          <Icon name="alert" size={15} />
          <span><b>{friendly.title}</b>{friendly.hint ? <small>{friendly.hint}</small> : null}</span>
          <button type="button" className="lv-chip" onClick={() => void controller.retry(answer)}>Try again</button>
          {friendly.screenPermission ? <button type="button" className="lv-chip" onClick={() => void controller.backend.openPrivacySettings("screen")}>Open Settings</button> : null}
          {friendly.settings ? <button type="button" className="lv-chip" onClick={() => void controller.backend.openMain()}>Open Aside</button> : null}
        </div>
      ) : answer.status === "done" && answer.canned ? (
        <div className="lv-problem">
          <Icon name="alert" size={15} />
          <span><b>Not a real answer</b><small>Aside couldn't reach the AI.</small></span>
          <button type="button" className="lv-chip" onClick={() => void controller.retry(answer)}>Try again</button>
        </div>
      ) : null}
    </article>
  );
}

/** The middle of the window: one question and its answer. Everything else in the window is a control. */
export function Reading() {
  const { page } = useAnswerPage();
  const capturing = useLive((state) => state.requestPending && state.nextSource === "screen");
  const resting = useLive((state) => state.channels.mic.status === "off" && state.channels.system.status === "off");
  const paused = useLiveUi((state) => state.paused !== null);
  const box = useRef<HTMLDivElement>(null);
  const id = page?.kind === "answer" ? page.answer.id : page?.question.turnId;
  useLayoutEffect(() => { if (box.current) box.current.scrollTop = 0; }, [id]);
  const question = page ? questionOf(page) : null;

  return (
    <div ref={box} className="lv-reading" tabIndex={0} aria-live="polite">
      {capturing ? (
        <p className="lv-quiet" role="status"><span className="lv-spin" aria-hidden="true" />Reading your screen…</p>
      ) : !page ? (
        <p className="lv-quiet">{paused || resting ? "Listening is paused." : "Listening. The next question shows here."}</p>
      ) : (
        <>
          {question ? <p className="lv-question selectable" data-waiting={page.kind === "question" || undefined}>{question}</p> : null}
          {page.kind === "answer" ? <AnswerText key={page.answer.id} answer={page.answer} /> : <p className="lv-quiet">Press Answer when you're ready.</p>}
        </>
      )}
    </div>
  );
}

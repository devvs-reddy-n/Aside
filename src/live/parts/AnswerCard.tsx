import { useState } from "react";
import { parseAnswerText } from "../../core/answerParse";
import { Icon } from "../../kit/Icon";
import { useController } from "../context";
import { describeAnswerError } from "../errors";
import type { Answer } from "../machine";
import { Code } from "./Code";

const TITLES: Record<Answer["source"], string> = {
  window: "Suggested answer",
  screen: "From your screen",
  typed: "Follow-up answer",
};

const LENGTHS: Record<Answer["length"], string> = { auto: "", small: "Short", medium: "Medium", long: "Long" };

const REFINEMENTS: Array<[string, string]> = [
  ["Shorter", "Give me a shorter version of that"],
  ["Add an example", "Add a concrete example to that answer"],
  ["Simpler words", "Say that again in simpler words"],
  ["As bullets", "Give me that as three short bullet points"],
];

/** While an answer streams, a code fence may not be closed yet. Close it so code renders as code the whole time. */
function forParsing(text: string, streaming: boolean): string {
  if (!streaming) return text;
  const fences = (text.match(/```/g) ?? []).length;
  return fences % 2 === 1 ? `${text}\n\`\`\`` : text;
}

export function answerToText(answer: Answer): string {
  const parsed = parseAnswerText(answer.text);
  const parts: string[] = [];
  if (parsed.prose) parts.push(parsed.prose);
  if (parsed.keySteps) parts.push(parsed.keySteps.map((step) => `- ${step}`).join("\n"));
  if (parsed.code) parts.push(`\`\`\`${parsed.code.language ?? ""}\n${parsed.code.code}\n\`\`\``);
  return parts.join("\n\n");
}

/** What was asked, in a short line: the cleaned-up question if Aside made one, otherwise the words as heard. */
export function askedLine(answer: Answer): string | null {
  const text = (answer.cleanedQuestion ?? answer.question).trim();
  return text || null;
}

interface AnswerCardProps {
  answer: Answer;
  /** The newest answer: open by default and offers the refine chips. */
  latest: boolean;
  /** Shown on its own (Focus / Prompter): never folds. */
  focused?: boolean;
  /** Briefly highlighted after stepping to it. */
  flash?: boolean;
  /** Prompter: large reading text, no card chrome. */
  large?: boolean;
}

/** One answer: its source, the text with steps and code, then what to do next (refine, retry or copy). */
export function AnswerCard({ answer, latest, focused = false, flash, large = false }: AnswerCardProps) {
  const controller = useController();
  const [copied, setCopied] = useState(false);
  /** null = follow the default (the newest answer is open, older ones fold up). */
  const [openChoice, setOpenChoice] = useState<boolean | null>(null);
  const streaming = answer.status === "streaming";
  const open = focused || (openChoice ?? latest);
  const parsed = parseAnswerText(forParsing(answer.text, streaming));
  const friendly = answer.error ? describeAnswerError(answer.error) : null;
  const foldable = !focused && !streaming && answer.status === "done" && !latest;

  function copy() {
    void navigator.clipboard
      .writeText(answerToText(answer))
      .then(() => {
        setCopied(true);
        window.setTimeout(() => setCopied(false), 1400);
      })
      .catch(() => undefined);
  }

  return (
    <article
      className="lv-answer"
      data-answer={answer.id}
      data-state={answer.status}
      data-flash={flash || undefined}
      data-large={large || undefined}
      aria-busy={streaming || undefined}
      aria-label={TITLES[answer.source]}
    >
      <header className="lv-answer__head">
        <span className="lv-answer__badge" aria-hidden="true"><Icon name={answer.source === "screen" ? "screen" : "sparkle"} size={14} strokeWidth={2} /></span>
        <b>{TITLES[answer.source]}</b>
        {LENGTHS[answer.length] ? <small>{LENGTHS[answer.length]}</small> : null}
        <span className="lv-grow" />
        {answer.status === "done" && answer.text ? (
          <button type="button" className="lv-iconbtn lv-iconbtn--sm" onClick={copy} aria-label={copied ? "Copied" : "Copy answer"} title="Copy answer">
            <Icon name={copied ? "check" : "copy"} size={15} />
          </button>
        ) : null}
      </header>

      <div className="lv-answer__body selectable" data-fold={foldable && !open ? "closed" : undefined}>
        {streaming && !answer.text ? <p className="lv-think" role="status"><i /><i /><i /><span>Thinking…</span></p> : null}
        {parsed.prose ? (
          <p className="lv-prose">
            {parsed.prose}
            {streaming && !parsed.code ? <span className="lv-caret" aria-hidden="true" /> : null}
          </p>
        ) : null}
        {parsed.keySteps ? <ul className="lv-steps">{parsed.keySteps.map((step, index) => <li key={index}>{step}</li>)}</ul> : null}
        {parsed.code ? <Code block={parsed.code} streaming={streaming} /> : null}
      </div>

      {foldable ? (
        <button type="button" className="lv-fold" aria-expanded={open} onClick={() => setOpenChoice(!open)}>
          <Icon name={open ? "chevronUp" : "chevronDown"} size={14} />{open ? "Show less" : "Show more"}
        </button>
      ) : null}

      {answer.status === "error" && friendly ? (
        <footer className="lv-answer__foot lv-answer__foot--problem">
          <span className="lv-oops"><Icon name="alert" size={15} /><span><b>{friendly.title}</b>{friendly.hint ? <small>{friendly.hint}</small> : null}</span></span>
          <button type="button" className="lv-chip" data-solid onClick={() => void controller.retry(answer)}><Icon name="refresh" size={14} />Try again</button>
          {friendly.screenPermission ? <button type="button" className="lv-chip" onClick={() => void controller.backend.openPrivacySettings("screen")}>Open System Settings</button> : null}
          {friendly.settings ? <button type="button" className="lv-chip" onClick={() => void controller.backend.openMain()}>Open Aside</button> : null}
        </footer>
      ) : answer.status === "done" && answer.canned ? (
        <footer className="lv-answer__foot lv-answer__foot--problem">
          <span className="lv-oops"><Icon name="alert" size={15} /><span><b>Stand-in message</b><small>Aside couldn't reach the AI, so this isn't a real answer.</small></span></span>
          <button type="button" className="lv-chip" data-solid onClick={() => void controller.retry(answer)}><Icon name="refresh" size={14} />Try again</button>
        </footer>
      ) : answer.status === "done" && latest ? (
        <footer className="lv-answer__foot lv-answer__foot--chips" aria-label="Refine this answer">
          {REFINEMENTS.map(([label, ask]) => (
            <button key={label} type="button" className="lv-chip" onClick={() => void controller.ask(ask)}>{label}</button>
          ))}
        </footer>
      ) : null}
    </article>
  );
}

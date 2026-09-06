import { useState } from "react";
import { parseAnswerText } from "./answerParse";
import { Icon } from "../../ui/Icon";

export interface OverlayAnswerCardProps {
  question: string | null;
  cleanedQuestion: string | null;
  text: string;
  streaming: boolean;
  canned: boolean;
  error: string | null;
}

function CodeCard({ language, code }: { language: string | null; code: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <div className="ov-code-card">
      <div className="ov-code-card__head">
        <span>
          <Icon name="code" />
          Code{language ? ` · ${language}` : ""}
        </span>
        <button
          type="button"
          className="ov-code-card__copy"
          onClick={() => {
            void navigator.clipboard.writeText(code).then(() => {
              setCopied(true);
              window.setTimeout(() => setCopied(false), 1500);
            });
          }}
        >
          {copied ? "Copied" : "Copy"}
        </button>
      </div>
      <pre>
        <code>{code}</code>
      </pre>
    </div>
  );
}

function QuestionHeader({ cleanedQuestion, question }: { cleanedQuestion: string | null; question: string | null }) {
  const shownQuestion = cleanedQuestion ?? question;
  if (!shownQuestion) return null;
  return (
    <div>
      <p className="ov-section__heading">
        <Icon name="message" />
        Question
      </p>
      <p className="ov-section__body">{shownQuestion}</p>
    </div>
  );
}

function AnswerBody({ prose, streaming, keySteps }: { prose: string; streaming: boolean; keySteps: string[] | null }) {
  // A coding/system-design answer now puts its whole explanation inside the
  // code block itself, as comments next to each part — nothing separate is
  // left to show here once streaming has actually finished. Still rendered
  // while streaming (there's normally text here before the closing code
  // fence arrives), just not left behind as an empty "Answer" heading with
  // nothing under it once the stream settles.
  if (!streaming && !prose.trim() && !keySteps) return null;
  return (
    <>
      <div>
        <p className="ov-section__heading ov-section__heading--answer">
          <Icon name="spark" />
          Answer
        </p>
        <p className="ov-section__body">
          {prose}
          {streaming ? <span className="ov-answer-cursor" aria-hidden="true" /> : null}
        </p>
      </div>

      {keySteps ? (
        <div>
          <p className="ov-section__heading">
            <Icon name="check" />
            Key Steps
          </p>
          <ul className="ov-key-steps">
            {keySteps.map((step, i) => (
              <li key={i}>{step}</li>
            ))}
          </ul>
        </div>
      ) : null}
    </>
  );
}

/**
 * Spec §6.3's answer card, restyled for dark glass, plus §6.4's coding
 * split. Reuses the same event-driven data (`useAnswerStream`) the
 * live-answer page's `AnswerCard.tsx` already streams from — this file
 * only changes how that data is parsed into sections and rendered, not
 * where it comes from.
 */
export function OverlayAnswerCard({
  question,
  cleanedQuestion,
  text,
  streaming,
  canned,
  error,
}: OverlayAnswerCardProps) {
  const hasContent = text.trim().length > 0 || question != null;

  if (!hasContent && !streaming) {
    // Spec §6.5: toolbar-only empty state has no body at all — the caller
    // (`OverlayWindow`) is responsible for not rendering this card in that
    // case. This branch only covers "a question exists but no text has
    // streamed in yet."
    return <p className="ov-card__empty">Nothing yet. Press ⌘↩ once a question has been asked.</p>;
  }

  const parsed = parseAnswerText(text);

  return (
    <>
      {canned ? <p className="ov-error-line">Could not reach the AI. This is a stand-in answer.</p> : null}
      {error ? <p className="ov-error-line">Problem: {error}</p> : null}

      {parsed.code ? (
        // Code before its explanation, both stacked vertically — matches
        // the adopted Stitch overlay mockup (code first, prose after), and
        // still never side-by-side.
        <div className="ov-coding-stack">
          <QuestionHeader cleanedQuestion={cleanedQuestion} question={question} />
          <CodeCard language={parsed.code.language} code={parsed.code.code} />
          <AnswerBody prose={parsed.prose} streaming={streaming} keySteps={parsed.keySteps} />
        </div>
      ) : (
        <>
          <QuestionHeader cleanedQuestion={cleanedQuestion} question={question} />
          <AnswerBody prose={parsed.prose} streaming={streaming} keySteps={parsed.keySteps} />
        </>
      )}
    </>
  );
}

import { useEffect, useRef, useState, type FormEvent, type ReactNode } from "react";
import type { AnswerLength } from "../../bridge/api";
import { Icon } from "../../kit/Icon";
import { useController, useLive, useLiveUi } from "../context";
import { isAnswering } from "../machine";

const LENGTHS: Array<[AnswerLength, string, string]> = [
  ["small", "Short", "Answer now, short"],
  ["medium", "Medium", "Answer now, medium length"],
  ["long", "Long", "Answer now, in detail"],
];

/** Type a follow-up question; Enter sends it. */
function AskBox({ autoFocus = false, onEscape }: { autoFocus?: boolean; onEscape?: () => void }) {
  const controller = useController();
  const answering = useLive(isAnswering);
  const [text, setText] = useState("");
  const input = useRef<HTMLInputElement>(null);

  useEffect(() => { if (autoFocus) input.current?.focus(); }, [autoFocus]);

  async function submit(event: FormEvent) {
    event.preventDefault();
    const question = text.trim();
    if (!question || answering) return;
    const ok = await controller.ask(question);
    if (ok) setText((current) => (current.trim() === question ? "" : current));
  }

  return (
    <form
      className="lv-ask"
      id="live-follow-up"
      onSubmit={(event) => void submit(event)}
      onKeyDown={(event) => {
        if (event.key === "Escape" && onEscape) {
          event.stopPropagation();
          onEscape();
        }
      }}
    >
      <Icon name="chat" size={15} />
      <input
        ref={input}
        value={text}
        onChange={(event) => setText(event.target.value)}
        placeholder="Ask a follow-up…"
        aria-label="Ask a follow-up question"
        autoComplete="off"
        spellCheck
      />
      <button type="submit" className="lv-send" disabled={!text.trim() || answering} aria-label="Send question" title="Send">
        <Icon name="send" size={15} strokeWidth={2.25} />
      </button>
    </form>
  );
}

function ScreenshotButton() {
  const controller = useController();
  const answering = useLive(isAnswering);
  const busy = useLiveUi((state) => state.screenshotBusy);
  return (
    <button type="button" className="lv-tool" disabled={answering || busy} onClick={() => void controller.screenshot()} aria-label="Answer from a screenshot" title="Answer from a screenshot  ⌘⇧↩">
      {busy ? <span className="lv-spin" aria-hidden="true" /> : <Icon name="camera" size={17} />}
    </button>
  );
}

/**
 * The bottom edge, always on screen: answer now (automatic length, or Short /
 * Medium / Long), answer from a screenshot, or type a follow-up.
 * `full` shows the follow-up box all the time; `slim` keeps one row and opens
 * the box on request.
 */
export function ActionBar({ mode = "full", lead }: { mode?: "full" | "slim"; lead?: ReactNode }) {
  const controller = useController();
  const answering = useLive(isAnswering);
  const [askOpen, setAskOpen] = useState(false);
  const askButton = useRef<HTMLButtonElement>(null);
  const slim = mode === "slim";

  return (
    <footer className="lv-actions" data-mode={mode}>
      {slim ? (askOpen ? <AskBox autoFocus onEscape={() => { setAskOpen(false); askButton.current?.focus(); }} /> : null) : (
        <div className="lv-actions__row">
          <AskBox />
          <ScreenshotButton />
        </div>
      )}
      <div className="lv-actions__row">
        {lead}
        <div className="lv-lengths" role="group" aria-label="Answer at a specific length">
          {LENGTHS.map(([value, label, title]) => (
            <button key={value} type="button" disabled={answering} title={title} aria-label={title} onClick={() => void controller.answer(value)}>
              <span className="lv-len-full">{label}</span><span className="lv-len-short" aria-hidden="true">{label[0]}</span>
            </button>
          ))}
        </div>
        {slim ? (
          <>
            <button ref={askButton} type="button" className="lv-tool" aria-label={askOpen ? "Close question box" : "Ask a follow-up"} title="Ask a follow-up" aria-expanded={askOpen} aria-controls="live-follow-up" onClick={() => setAskOpen((open) => !open)}>
              <Icon name={askOpen ? "x" : "chat"} size={17} />
            </button>
            <ScreenshotButton />
          </>
        ) : null}
        <button
          type="button"
          className="lv-go"
          disabled={answering}
          aria-busy={answering || undefined}
          aria-label="Answer now, length adapts to the question"
          title="Answer now — length adapts to the question  ⌘↩"
          onClick={() => void controller.answer()}
        >
          {answering ? <span className="lv-spin" aria-hidden="true" /> : <Icon name="play" size={16} strokeWidth={2.25} />}
          <span className="lv-go__label">{answering ? "Answering" : "Answer"}</span>
          {answering ? null : <kbd>⌘↩</kbd>}
        </button>
      </div>
    </footer>
  );
}

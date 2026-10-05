import { useState, type FormEvent } from "react";
import { Icon } from "../../kit/Icon";
import { useController, useLive, useLiveUi } from "../context";
import { isAnswering } from "../machine";

/**
 * The bottom bar, one row: type a follow-up (Enter sends), answer from a
 * screenshot, or answer what was just asked.
 */
export function Composer() {
  const controller = useController();
  const answering = useLive(isAnswering);
  const shooting = useLiveUi((state) => state.screenshotBusy);
  const [text, setText] = useState("");

  async function submit(event: FormEvent) {
    event.preventDefault();
    const question = text.trim();
    if (!question || answering) return;
    const ok = await controller.ask(question);
    if (ok) setText((current) => (current.trim() === question ? "" : current));
  }

  return (
    <footer className="lv-composer">
      <form className="lv-ask" onSubmit={(event) => void submit(event)}>
        <input
          value={text}
          onChange={(event) => setText(event.target.value)}
          placeholder="Ask a follow-up"
          aria-label="Ask a follow-up question"
          autoComplete="off"
          spellCheck
        />
        {text.trim() ? (
          <button type="submit" className="lv-iconbtn" disabled={answering} aria-label="Send question" title="Send  ↩">
            <Icon name="send" size={15} strokeWidth={2.25} />
          </button>
        ) : null}
      </form>
      <button type="button" className="lv-iconbtn lv-iconbtn--field" disabled={answering || shooting} onClick={() => void controller.screenshot()} aria-label="Answer from a screenshot" title="Answer from a screenshot  ⌘⇧↩">
        {shooting ? <span className="lv-spin" aria-hidden="true" /> : <Icon name="camera" size={16} />}
      </button>
      <button
        type="button"
        className="lv-go"
        disabled={answering}
        aria-busy={answering || undefined}
        aria-label="Answer now"
        title="Answer now  ⌘↩"
        onClick={() => void controller.answer()}
      >
        {answering ? <span className="lv-spin" aria-hidden="true" /> : null}
        {answering ? "Answering" : "Answer"}
      </button>
    </footer>
  );
}

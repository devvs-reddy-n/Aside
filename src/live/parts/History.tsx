import { PREVIOUS_SHORTCUT, NEXT_SHORTCUT } from "../../core/shortcuts";
import { Icon } from "../../kit/Icon";
import { useController } from "../context";
import { useAnswerPage } from "./status";

/** ‹ 2 / 5 ›: step through earlier questions and answers (also ⌘< and ⌘>). */
export function AnswerHistory() {
  const controller = useController();
  const { index, count } = useAnswerPage();
  if (count < 2) return null;
  return (
    <nav className="lv-history" aria-label="Answer history">
      <button type="button" className="lv-iconbtn" aria-label="Previous answer" title={`Previous answer  ${PREVIOUS_SHORTCUT}`} disabled={index <= 0} onClick={() => controller.stepAnswer(-1)}><Icon name="chevronLeft" size={15} /></button>
      <span className="tabular" aria-live="polite">{index + 1}/{count}</span>
      <button type="button" className="lv-iconbtn" aria-label="Next answer" title={`Next answer  ${NEXT_SHORTCUT}`} disabled={index >= count - 1} onClick={() => controller.stepAnswer(1)}><Icon name="chevronRight" size={15} /></button>
    </nav>
  );
}


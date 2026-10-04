import { useLayoutEffect, useRef } from "react";
import { Icon } from "../../kit/Icon";
import { useController, useLive, useLiveUi } from "../context";
import { ActionBar } from "../parts/ActionBar";
import { Alerts } from "../parts/Alerts";
import { AnswerCard } from "../parts/AnswerCard";
import { AudioLane } from "../parts/AudioRow";
import { Conversation } from "../parts/Conversation";
import { AnswerNav, QuestionCard } from "../parts/QuestionCard";
import { useAnswerPage } from "../parts/status";
import { TopBar } from "../parts/TopBar";

/** Large answer text with the question on one line above it. */
function Reading() {
  const { page, pages } = useAnswerPage();
  const pending = useLive((state) => state.requestPending);
  const screen = useLive((state) => state.nextSource === "screen");
  const box = useRef<HTMLDivElement>(null);
  const id = page?.kind === "answer" ? page.answer.id : page?.question.turnId;
  useLayoutEffect(() => { if (box.current) box.current.scrollTop = 0; }, [id]);

  return (
    <div className="lv-scrollwrap" aria-label="Answer">
      <div ref={box} className="lv-scroll lv-reading" tabIndex={0} aria-live="polite">
        <QuestionCard page={page} oneLine={page?.kind === "answer"} />
        {pending && screen ? (
          <p className="lv-capturing" role="status"><span className="lv-spin" aria-hidden="true" />Reading your screen…</p>
        ) : page?.kind === "answer" ? (
          <AnswerCard key={page.answer.id} answer={page.answer} latest={page === pages.at(-1)} focused large />
        ) : null}
      </div>
    </div>
  );
}

/** Teleprompter: built for reading on camera. The transcript opens as a drawer from the bottom. */
export function Prompter() {
  const controller = useController();
  const drawer = useLiveUi((state) => state.view === "all");
  const toggle = (
    <button
      type="button"
      className="lv-tool"
      aria-expanded={drawer}
      aria-controls="live-feed-panel"
      aria-label={drawer ? "Hide transcript" : "Show transcript"}
      title={drawer ? "Hide transcript" : "Show transcript"}
      onClick={() => controller.setView(drawer ? "answers" : "all")}
    >
      <Icon name={drawer ? "chevronDown" : "list"} size={17} />
    </button>
  );

  return (
    <>
      <TopBar slim>
        <AudioLane channel="mic" iconOnly />
        <AudioLane channel="system" iconOnly />
        <AnswerNav compact />
      </TopBar>
      <Alerts />
      <div className="lv-prompter">
        <Reading />
        {drawer ? (
          <div className="lv-drawer">
            <Conversation answers={false} />
          </div>
        ) : null}
      </div>
      <ActionBar mode="slim" lead={toggle} />
    </>
  );
}

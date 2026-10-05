import { motion } from "motion/react";
import { useLayoutEffect, useRef } from "react";
import { SPRING } from "../../kit/motion";
import { Icon } from "../../kit/Icon";
import { useController, useLive, useLiveUi } from "../context";
import type { Room } from "../hooks";
import { ActionBar } from "../parts/ActionBar";
import { Alerts } from "../parts/Alerts";
import { AnswerCard } from "../parts/AnswerCard";
import { AudioLane, AudioRow } from "../parts/AudioRow";
import { Conversation } from "../parts/Conversation";
import { AnswerNav, QuestionCard } from "../parts/QuestionCard";
import { useAnswerPage } from "../parts/status";
import { TopBar } from "../parts/TopBar";

/** Answer | Transcript. Arrow keys move between the two. */
function Tabs() {
  const controller = useController();
  const view = useLiveUi((state) => state.view);
  return (
    <div className="lv-tabsrow">
      <div className="lv-tabs" role="tablist" aria-label="What to show" onKeyDown={(event) => {
        if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
        event.preventDefault();
        const next = event.key === "Home" ? "answers" : event.key === "End" ? "all" : view === "answers" ? "all" : "answers";
        controller.setView(next);
        event.currentTarget.querySelector<HTMLButtonElement>(`[data-view="${next}"]`)?.focus();
      }}>
        <button type="button" role="tab" data-view="answers" aria-controls="live-feed-panel" tabIndex={view === "answers" ? 0 : -1} aria-selected={view === "answers"} onClick={() => controller.setView("answers")}>
          {view === "answers" ? <motion.span layoutId="lv-tab-thumb" className="lv-tabs__thumb" transition={SPRING} /> : null}
          <Icon name="sparkle" size={14} /><span>Answer</span>
        </button>
        <button type="button" role="tab" data-view="all" aria-controls="live-feed-panel" tabIndex={view === "all" ? 0 : -1} aria-selected={view === "all"} onClick={() => controller.setView("all")}>
          {view === "all" ? <motion.span layoutId="lv-tab-thumb" className="lv-tabs__thumb" transition={SPRING} /> : null}
          <Icon name="chat" size={14} /><span>Transcript</span>
        </button>
      </div>
      {view === "answers" ? <AnswerNav /> : null}
    </div>
  );
}

/** The question being answered, then its answer. One page at a time. */
function QuestionAndAnswer() {
  const { page, pages } = useAnswerPage();
  const pending = useLive((state) => state.requestPending);
  const screen = useLive((state) => state.nextSource === "screen");
  const box = useRef<HTMLDivElement>(null);
  const id = page?.kind === "answer" ? page.answer.id : page?.question.turnId;
  useLayoutEffect(() => { if (box.current) box.current.scrollTop = 0; }, [id]);
  const capturing = pending && screen;

  return (
    <div className="lv-scrollwrap" role="tabpanel" id="live-feed-panel" aria-label="Question and answer">
      <div ref={box} className="lv-scroll lv-stack" tabIndex={0} aria-live="polite">
        {capturing ? (
          <p className="lv-capturing" role="status"><span className="lv-spin" aria-hidden="true" />Reading your screen…</p>
        ) : (
          <>
            <QuestionCard page={page} />
            {page?.kind === "answer" ? <AnswerCard key={page.answer.id} answer={page.answer} latest={page === pages.at(-1)} focused /> : null}
          </>
        )}
      </div>
    </div>
  );
}

/**
 * Focus stack: status, both sides of the call, the question, its answer, then
 * one clear action bar. In a tiny window You and Them move into the top bar as
 * icons and the tabs step aside, so the answer keeps most of the room.
 */
export function Focus({ room }: { room: Room }) {
  const view = useLiveUi((state) => state.view);
  const tiny = room.height === "tiny";
  return (
    <>
      {tiny ? (
        <TopBar slim start={<><AudioLane channel="mic" iconOnly /><AudioLane channel="system" iconOnly /></>} end={<AnswerNav compact />} />
      ) : (
        <>
          <TopBar />
          <AudioRow />
        </>
      )}
      <Alerts />
      {tiny ? null : <Tabs />}
      {view === "answers" || tiny ? <QuestionAndAnswer /> : <Conversation answers={false} />}
      <ActionBar mode={room.height === "regular" ? "full" : "slim"} />
    </>
  );
}

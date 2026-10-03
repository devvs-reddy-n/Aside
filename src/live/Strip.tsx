import { Icon } from "../kit/Icon";
import { answerPages, pageIndex } from "./navigation";
import { useController, useLive, useLiveUi } from "./context";
import { channelHasActivity, type ChannelKey, type ChannelView } from "./machine";


const NAMES: Record<ChannelKey, { name: string; icon: "mic" | "speaker"; help: string }> = {
  mic: { name: "You", icon: "mic", help: "your microphone" },
  system: { name: "Them", icon: "speaker", help: "the other side of the call" },
};

function stateLabel(view: ChannelView): string {
  if (channelHasActivity(view)) return "sound detected";
  switch (view.status) {
    case "off": return "off";
    case "starting": return "starting";
    case "silent": return "quiet";
    case "error": return "problem";
    default: return view.speaking ? "speaking" : "on";
  }
}

/** Icon-only audio control in the header, with its state and action read aloud. */
export function AudioControl({ channel }: { channel: ChannelKey }) {
  const controller = useController();
  const view = useLive((state) => state.channels[channel]);
  const { name, icon, help } = NAMES[channel];
  const on = view.status === "on" || view.status === "silent";
  return (
    <button
      type="button"
      className="lv-lane lv-audio-icon"
      data-status={view.status}
      data-active={channelHasActivity(view) || undefined}
      aria-pressed={on}
      disabled={view.status === "starting"}
      title={`${name}: ${stateLabel(view)}. Tap to turn ${on ? "off" : "on"} ${help}.`}
      aria-label={`${name}, ${stateLabel(view)}. Turn ${on ? "off" : "on"} ${help}.`}
      onClick={() => void controller.toggleChannel(channel)}
    >
      {view.status === "starting" ? <span className="lv-spin" aria-hidden="true" /> : <Icon name={icon} size={16} />}
      {view.status === "off" ? <span className="lv-audio-slash" aria-hidden="true" /> : null}
    </button>
  );
}

/** Mode control lives in the header, replacing the company name. */
export function ViewTabs() {
  const controller = useController();
  const view = useLiveUi((state) => state.view);
  return <div className="lv-tabs" role="tablist" aria-label="What to show" onKeyDown={(event) => {
    if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
    event.preventDefault();
    const next = event.key === "Home" ? "answers" : event.key === "End" ? "all" : view === "answers" ? "all" : "answers";
    controller.setView(next);
    event.currentTarget.querySelector<HTMLButtonElement>(`[data-view="${next}"]`)?.focus();
  }}>
    <button type="button" role="tab" id="live-answers-tab" aria-controls="live-feed-panel" data-view="answers" tabIndex={view === "answers" ? 0 : -1} aria-selected={view === "answers"} onClick={() => controller.setView("answers")} title="One question and answer at a time">Answer</button>
    <button type="button" role="tab" id="live-chat-tab" aria-controls="live-feed-panel" data-view="all" tabIndex={view === "all" ? 0 : -1} aria-selected={view === "all"} onClick={() => controller.setView("all")} title="Live conversation and answers">Chat</button>
  </div>;
}

/** Only reserve a history row when there is something to navigate. */
export function Strip() {
  const controller = useController();
  const view = useLiveUi((state) => state.view);
  const state = useLive((state) => ({ items: state.items, question: state.question }));
  const cursor = useLiveUi((state) => state.cursor);
  const count = answerPages(state).length;
  const index = pageIndex(count, cursor?.index);
  if (view !== "answers" || count < 2) return null;
  return <div className="lv-strip">
    <nav className="lv-answer-nav" aria-label="Answer history">
      <button type="button" className="lv-icon lv-icon--sm" aria-label="Previous answer" disabled={index <= 0} onClick={() => controller.stepAnswer(-1)}><Icon name="chevronLeft" size={16} /></button>
      <span className="tabular" aria-live="polite">{`${index + 1} / ${count}`}</span>
      <button type="button" className="lv-icon lv-icon--sm" aria-label="Next answer" disabled={index >= count - 1} onClick={() => controller.stepAnswer(1)}><Icon name="chevronRight" size={16} /></button>
    </nav>
  </div>;
}

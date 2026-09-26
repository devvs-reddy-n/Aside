import { Icon } from "../kit/Icon";
import { LevelBars } from "../kit/LevelBars";
import { useController, useLive, useLiveUi } from "./context";
import type { ChannelKey, ChannelView } from "./machine";
import type { Room } from "./hooks";

const NAMES: Record<ChannelKey, { name: string; icon: "mic" | "speaker"; help: string }> = {
  mic: { name: "You", icon: "mic", help: "your microphone" },
  system: { name: "Them", icon: "speaker", help: "the other side of the call" },
};

function stateLabel(view: ChannelView): string {
  switch (view.status) {
    case "off": return "off";
    case "starting": return "starting";
    case "silent": return "quiet";
    case "error": return "problem";
    default: return view.speaking ? "speaking" : "on";
  }
}

/** One side of the call: its name and a live meter. Tapping it turns that side off or on. */
function Lane({ channel }: { channel: ChannelKey }) {
  const controller = useController();
  const view = useLive((state) => state.channels[channel]);
  const { name, icon, help } = NAMES[channel];
  const on = view.status === "on" || view.status === "silent";
  return (
    <button
      type="button"
      className="lv-lane"
      data-status={view.status}
      aria-pressed={on}
      disabled={view.status === "starting"}
      title={`${name}: ${stateLabel(view)}. Tap to turn ${on ? "off" : "on"} ${help}.`}
      aria-label={`${name}, ${stateLabel(view)}. Turn ${on ? "off" : "on"} ${help}.`}
      onClick={() => void controller.toggleChannel(channel)}
    >
      {view.status === "starting" ? <span className="lv-spin" aria-hidden="true" /> : <Icon name={view.status === "off" ? "minus" : icon} size={14} />}
      <span>{name}</span>
      <LevelBars level={view.level} active={view.status === "on"} />
    </button>
  );
}

/** Row two: the two sides of the call, then what the feed shows. */
export function Strip({ room }: { room: Room }) {
  const controller = useController();
  const view = useLiveUi((state) => state.view);
  return (
    <div className="lv-strip" data-narrow={room.width === "narrow" || undefined}>
      <Lane channel="mic" />
      <Lane channel="system" />
      <span className="lv-grow" />
      <div className="lv-tabs" role="tablist" aria-label="What to show">
        <button type="button" role="tab" aria-selected={view === "answers"} onClick={() => controller.setView("answers")} title="Only Aside's answers">Answers</button>
        <button type="button" role="tab" aria-selected={view === "all"} onClick={() => controller.setView("all")} title="What was said, with answers in place">Transcript</button>
      </div>
    </div>
  );
}

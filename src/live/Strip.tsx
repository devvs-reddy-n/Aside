import { Icon } from "../kit/Icon";
import { useController, useLive, useLiveUi } from "./context";
import type { ChannelKey, ChannelView } from "./machine";
import type { Room } from "./hooks";

const NAMES: Record<ChannelKey, { name: string; icon: "mic" | "speaker"; help: string }> = {
  mic: { name: "You", icon: "mic", help: "your microphone" },
  system: { name: "Others", icon: "speaker", help: "the other side of the call" },
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

/** Three little bars that rise and fall with the sound this side is hearing. */
function Meter({ level, live }: { level: number; live: boolean }) {
  const base = live ? Math.min(1, Math.sqrt(level) * 3.4) : 0;
  const heights = [0.55, 1, 0.75].map((weight) => 0.16 + base * weight * 0.84);
  return (
    <span className="lv-meter" aria-hidden="true">
      {heights.map((height, index) => <i key={index} style={{ transform: `scaleY(${height.toFixed(3)})` }} />)}
    </span>
  );
}

export function ChannelChip({ channel, showName = true }: { channel: ChannelKey; showName?: boolean }) {
  const controller = useController();
  const view = useLive((state) => state.channels[channel]);
  const { name, icon, help } = NAMES[channel];
  const on = view.status === "on" || view.status === "silent";
  return (
    <button
      type="button"
      className="lv-chan"
      data-status={view.status}
      data-speaking={view.speaking || undefined}
      aria-pressed={on}
      disabled={view.status === "starting"}
      title={`${name}: ${stateLabel(view)} — ${on ? "tap to turn off" : "tap to turn on"} (${help})`}
      aria-label={`${name}, ${stateLabel(view)}. ${on ? "Turn off" : "Turn on"} ${help}.`}
      onClick={() => void controller.toggleChannel(channel)}
    >
      {view.status === "starting" ? <span className="lv-spin" aria-hidden="true" /> : <Icon name={icon} size={15} />}
      {showName ? <span className="lv-chan__name">{name}</span> : null}
      <Meter level={view.level} live={view.status === "on"} />
      {showName ? <span className="lv-chan__state">{stateLabel(view)}</span> : null}
    </button>
  );
}

/** The two ears and the view switch, on one calm row under the header. */
export function Strip({ room }: { room: Room }) {
  const controller = useController();
  const view = useLiveUi((state) => state.view);
  const wide = room.width === "wide";
  return (
    <div className="lv-strip">
      <ChannelChip channel="mic" />
      <ChannelChip channel="system" />
      <span className="lv-grow" />
      <div className="lv-seg" role="group" aria-label="What to show">
        <button type="button" aria-pressed={view === "all"} onClick={() => controller.setView("all")} title="Show the conversation and answers">
          <Icon name="list" size={14} />{wide ? <span>All</span> : null}
        </button>
        <button type="button" aria-pressed={view === "answers"} onClick={() => controller.setView("answers")} title="Show only answers">
          <Icon name="sparkle" size={14} />{wide ? <span>Answers</span> : null}
        </button>
      </div>
    </div>
  );
}

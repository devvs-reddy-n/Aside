import { Icon } from "../../kit/Icon";
import { LevelBars } from "../../kit/LevelBars";
import { useController, useLive } from "../context";
import { channelHasActivity, type ChannelKey, type ChannelView } from "../machine";

const LANES: Record<ChannelKey, { name: string; icon: "mic" | "speaker"; help: string }> = {
  mic: { name: "You", icon: "mic", help: "your microphone" },
  system: { name: "Them", icon: "speaker", help: "the other side of the call" },
};

function stateWord(view: ChannelView): string | null {
  switch (view.status) {
    case "off": return "Off";
    case "starting": return "Starting";
    case "silent": return "Quiet";
    case "error": return "Problem";
    default: return null;
  }
}

function spoken(view: ChannelView): string {
  if (channelHasActivity(view)) return "sound detected";
  return stateWord(view)?.toLowerCase() ?? (view.speaking ? "speaking" : "on");
}

/** One side of the call: its name, a live level meter and its state. Tap to turn it on or off. */
export function AudioLane({ channel, iconOnly = false }: { channel: ChannelKey; iconOnly?: boolean }) {
  const controller = useController();
  const view = useLive((state) => state.channels[channel]);
  const { name, icon, help } = LANES[channel];
  const on = view.status === "on" || view.status === "silent";
  const word = stateWord(view);
  return (
    <button
      type="button"
      className="lv-lane"
      data-status={view.status}
      data-active={channelHasActivity(view) || undefined}
      data-icon-only={iconOnly || undefined}
      aria-pressed={on}
      disabled={view.status === "starting"}
      title={`${name}: ${spoken(view)}. Tap to turn ${on ? "off" : "on"} ${help}.`}
      aria-label={`${name}, ${spoken(view)}. Turn ${on ? "off" : "on"} ${help}.`}
      onClick={() => void controller.toggleChannel(channel)}
    >
      {view.status === "starting" ? <span className="lv-spin" aria-hidden="true" /> : <Icon name={icon} size={15} strokeWidth={2} />}
      {iconOnly ? null : <span className="lv-lane__name">{name}</span>}
      {iconOnly ? null : <LevelBars level={view.level} active={on} />}
      {!iconOnly && word ? <small className="lv-lane__state">{word}</small> : null}
      {view.status === "off" ? <span className="lv-lane__slash" aria-hidden="true" /> : null}
    </button>
  );
}

/** Both sides of the call, side by side. */
export function AudioRow({ compact = false }: { compact?: boolean }) {
  return (
    <div className="lv-audio" role="group" aria-label="Audio">
      <AudioLane channel="mic" iconOnly={compact} />
      <AudioLane channel="system" iconOnly={compact} />
    </div>
  );
}

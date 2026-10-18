import { useCallback, useRef, useState } from "react";
import { Icon } from "../../kit/Icon";
import { StealthStatus } from "../../kit/Stealth";
import { useController, useLive } from "../context";
import { noMaximize, useElapsed } from "../hooks";
import { channelHasActivity, type ChannelKey } from "../machine";
import { End } from "./End";
import { Menu } from "./Menu";
import { usePhase } from "./status";

const EARS: Record<ChannelKey, { name: string; icon: "mic" | "speaker"; help: string }> = {
  mic: { name: "You", icon: "mic", help: "your microphone" },
  system: { name: "Them", icon: "speaker", help: "the other side of the call" },
};

/** One side of the call as a small icon button. It brightens while sound comes in; tap to turn it on or off. */
function Ear({ channel }: { channel: ChannelKey }) {
  const controller = useController();
  const view = useLive((state) => state.channels[channel]);
  const { name, icon, help } = EARS[channel];
  const on = view.status === "on" || view.status === "silent";
  const heard = channelHasActivity(view);
  const state = view.status === "off" ? "off" : view.status === "error" ? "not working" : view.status === "starting" ? "starting" : heard ? "hearing sound" : "on";
  return (
    <button
      type="button"
      className="lv-ear"
      data-status={view.status}
      data-heard={heard || undefined}
      aria-pressed={on}
      disabled={view.status === "starting"}
      aria-label={`${name}, ${state}. Turn ${on ? "off" : "on"} ${help}.`}
      title={`${name}: ${state}`}
      onClick={() => void controller.toggleChannel(channel)}
    >
      {view.status === "starting" ? <span className="lv-spin" aria-hidden="true" /> : <Icon name={icon} size={15} strokeWidth={2} />}
      {view.status === "off" ? <span className="lv-ear__slash" aria-hidden="true" /> : null}
    </button>
  );
}

/**
 * The single top bar: a status dot with the call clock, You and Them, answer
 * window options and End. Empty space drags the window.
 */
export function Bar() {
  const elapsed = useElapsed();
  const { tone, text } = usePhase();
  const [menuOpen, setMenuOpen] = useState(false);
  const closeMenu = useCallback(() => setMenuOpen(false), []);
  const menuButton = useRef<HTMLButtonElement>(null);

  return (
    <header className="lv-bar" data-tauri-drag-region {...noMaximize}>
      <span className="lv-status" data-tone={tone} role="status" title={text} aria-label={`${text}. Time on this call ${elapsed}`} data-tauri-drag-region>
        <i className="lv-status__dot" aria-hidden="true" />
        <time className="tabular" data-tauri-drag-region>{elapsed}</time>
      </span>
      <StealthStatus compact />
      <div className="lv-audio" role="group" aria-label="Listening controls">
        <Ear channel="mic" />
        <Ear channel="system" />
      </div>
      <span className="lv-grow" data-tauri-drag-region />
      <button
        ref={menuButton}
        type="button"
        className="lv-iconbtn"
        aria-label="Window options"
        title="Window options"
        aria-haspopup="dialog"
        aria-expanded={menuOpen}
        onClick={() => setMenuOpen((open) => !open)}
      >
        <Icon name="more" size={17} />
      </button>
      <End />
      {menuOpen ? <Menu anchor={menuButton} onClose={closeMenu} /> : null}
    </header>
  );
}

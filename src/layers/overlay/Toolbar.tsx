import { useEffect, useRef, useState } from "react";
import type { AudioChannelState } from "../../state/audioChannel";
import { Icon } from "../../ui/Icon";

export interface ToolbarProps {
  listening: boolean;
  mic: AudioChannelState;
  system: AudioChannelState;
  onToggleMic: () => void;
  onToggleSystem: () => void;
  onAnswer: () => void;
  onAnswerLength: (length: "small" | "medium" | "long") => void;
  answering: boolean;
  onScreenshot: () => void;
  screenshotBusy: boolean;
  chatOpen: boolean;
  onToggleChat: () => void;
  opacityPct: number;
  onOpacityChange: (next: number) => void;
  collapsed: boolean;
  onToggleCollapse: () => void;
  moreOpen: boolean;
  onToggleMore: () => void;
  moreButtonRef: React.RefObject<HTMLButtonElement | null>;
  elapsedLabel: string;
  onEndClick: () => void;
}

/**
 * The transparency slider, always visible in the toolbar itself rather than
 * tucked in the More menu (2026-08-30, owner request — the Stitch source's
 * own Live Overlay mockup keeps this one control inline in the pill row,
 * everything else in the overflow menu). Same debounced-commit shape as the
 * More menu's own `Slider`: the thumb tracks every pointer move locally, a
 * real `set_overlay_prefs` call only fires after a short pause.
 */
function OpacitySlider({ value, onChange }: { value: number; onChange: (next: number) => void }) {
  const [local, setLocal] = useState(value);
  const commitTimer = useRef<number | null>(null);

  useEffect(() => setLocal(value), [value]);
  useEffect(() => () => {
    if (commitTimer.current != null) window.clearTimeout(commitTimer.current);
  }, []);

  function handleInput(next: number) {
    setLocal(next);
    if (commitTimer.current != null) window.clearTimeout(commitTimer.current);
    commitTimer.current = window.setTimeout(() => onChange(next), 120);
  }

  return (
    <span className="ov-pill ov-pill--opacity" title={`Transparency: ${local}%`}>
      <Icon name="opacity" />
      <input
        type="range"
        className="ov-pill-slider"
        aria-label="Transparency"
        min={55}
        max={95}
        step={5}
        value={local}
        onChange={(event) => handleInput(Number(event.target.value))}
      />
      <span className="numeral ov-pill-slider__value">{local}%</span>
    </span>
  );
}

function earState(channel: AudioChannelState): "capturing" | "error" | "idle" {
  if (channel.error) return "error";
  if (channel.capturing) return "capturing";
  return "idle";
}

function earTooltip(label: string, channel: AudioChannelState): string {
  if (channel.error) return `${label}: ${channel.error}`;
  if (channel.capturing) return `${label}: ${channel.info?.device_name ?? "listening"}`;
  return `${label}: not listening`;
}

/**
 * Quick device switch, right in the toolbar next to the mic/speaker icons
 * (2026-08-31, moved out of Settings — mid-call is where you'd actually
 * want to change this). Reuses `AudioChannelState.devices`/`chooseDevice`
 * exactly as Settings' old device pickers did; no new backend needed —
 * `chooseDevice` already persists the pick and, if already capturing, does
 * a clean stop/restart on the new device.
 */
function DevicePicker({
  label,
  channel,
  open,
  onToggle,
  onClose,
}: {
  label: string;
  channel: AudioChannelState;
  open: boolean;
  onToggle: () => void;
  onClose: () => void;
}) {
  return (
    <span className="ov-device-picker">
      <button
        type="button"
        className="ov-device-picker__trigger"
        title={`Choose ${label.toLowerCase()} device`}
        aria-label={`Choose ${label.toLowerCase()} device`}
        aria-expanded={open}
        onClick={onToggle}
      >
        <Icon name="chevronDown" size={10} />
      </button>
      {open ? (
        <div className="ov-device-picker__menu" role="menu">
          <button
            type="button"
            role="menuitemradio"
            aria-checked={channel.deviceId == null}
            className={`ov-device-picker__item${channel.deviceId == null ? " ov-device-picker__item--active" : ""}`}
            onClick={() => {
              void channel.chooseDevice(null);
              onClose();
            }}
          >
            Use the computer's default
          </button>
          {channel.devices
            .filter((device) => device.id != null)
            .map((device) => (
              <button
                type="button"
                role="menuitemradio"
                aria-checked={channel.deviceId === device.id}
                key={device.id}
                className={`ov-device-picker__item${channel.deviceId === device.id ? " ov-device-picker__item--active" : ""}`}
                onClick={() => {
                  void channel.chooseDevice(device.id as string);
                  onClose();
                }}
              >
                {device.name}
                {device.is_default ? " (default)" : ""}
              </button>
            ))}
        </div>
      ) : null}
    </span>
  );
}

const LENGTH_OPTIONS: Array<{ value: "small" | "medium" | "long"; letter: string; label: string; hint: string }> = [
  { value: "small", letter: "S", label: "Small", hint: "Small answer (~15-40 words)" },
  { value: "medium", letter: "M", label: "Medium", hint: "Medium answer (~40-90 words)" },
  { value: "long", letter: "L", label: "Long", hint: "Long answer (~90-180 words)" },
];

/**
 * Three extra ways to trigger the same answer as the Answer button, each
 * asking for a specific length instead of letting the AI judge it (2026-08-31,
 * matching the reference app's Small/Medium/Long transcript buttons). Pressing
 * one answers the current conversation immediately — it is not a toggle that
 * changes what the next Answer press does, so no button here ever shows
 * "active": every press is its own one-shot request.
 */
function LengthButtons({
  onPick,
  disabled,
}: {
  onPick: (length: "small" | "medium" | "long") => void;
  disabled: boolean;
}) {
  return (
    <span className="ov-length-group" role="group" aria-label="Answer with a specific length">
      {LENGTH_OPTIONS.map((option) => (
        <button
          key={option.value}
          type="button"
          className="ov-length-btn"
          title={option.hint}
          aria-label={option.label}
          onClick={() => onPick(option.value)}
          disabled={disabled}
        >
          {option.letter}
        </button>
      ))}
    </span>
  );
}

/**
 * The overlay's one always-visible bar (DESIGN.md §2 — this bar is the L4
 * "nearest" plane, the whole point of the ladder: it is the surface closest
 * to the person, floating directly over a live call). Every button inside
 * it is `.ov-pill`, an L2 control resting on that plane — no blur of its
 * own, only the inset top-highlight the design system gives L2 (§2). Press
 * feedback is `scale(.97)` at `--t-press` (§8); nothing here animates in
 * response to a *keyboard* shortcut firing, only its result may (design
 * brief: shortcuts fire potentially hundreds of times a call).
 */
export function Toolbar({
  listening,
  mic,
  system,
  onToggleMic,
  onToggleSystem,
  onAnswer,
  onAnswerLength,
  answering,
  onScreenshot,
  screenshotBusy,
  chatOpen,
  onToggleChat,
  opacityPct,
  onOpacityChange,
  collapsed,
  onToggleCollapse,
  moreOpen,
  onToggleMore,
  moreButtonRef,
  elapsedLabel,
  onEndClick,
}: ToolbarProps) {
  const [openPicker, setOpenPicker] = useState<"mic" | "system" | null>(null);

  return (
    <div className="ov-toolbar" data-tauri-drag-region>
      <Icon name="waveform" className={`ov-waveform${listening ? " ov-waveform--listening" : ""}`} />

      <span className="ov-ears" aria-label="Audio controls">
        <button
          type="button"
          className={`ov-ear-toggle ov-ear-toggle--${earState(mic)}`}
          title={earTooltip("Microphone", mic)}
          aria-label={`${mic.capturing ? "Stop" : "Start"} microphone`}
          aria-pressed={mic.capturing}
          onClick={onToggleMic}
          disabled={mic.busy !== null}
        >
          <Icon name="mic" />
        </button>
        <DevicePicker
          label="Microphone"
          channel={mic}
          open={openPicker === "mic"}
          onToggle={() => setOpenPicker((current) => (current === "mic" ? null : "mic"))}
          onClose={() => setOpenPicker(null)}
        />
        <button
          type="button"
          className={`ov-ear-toggle ov-ear-toggle--${earState(system)}`}
          title={earTooltip("System audio", system)}
          aria-label={`${system.capturing ? "Stop" : "Start"} system audio`}
          aria-pressed={system.capturing}
          onClick={onToggleSystem}
          disabled={system.busy !== null}
        >
          <Icon name="speaker" />
        </button>
        <DevicePicker
          label="System audio"
          channel={system}
          open={openPicker === "system"}
          onToggle={() => setOpenPicker((current) => (current === "system" ? null : "system"))}
          onClose={() => setOpenPicker(null)}
        />
      </span>

      <button type="button" className="ov-pill ov-pill--primary" title="Answer (⌘↩)" onClick={onAnswer} disabled={answering}>
        <Icon name="spark" />
        {answering ? "Working…" : "Answer"}
      </button>

      <LengthButtons onPick={onAnswerLength} disabled={answering} />

      <button type="button" className="ov-pill" title="Screenshot (⌘K)" onClick={onScreenshot} disabled={screenshotBusy}>
        <Icon name="camera" />
        {screenshotBusy ? "Looking…" : "Screenshot"}
      </button>

      <button
        type="button"
        className={`ov-pill${chatOpen ? " ov-pill--active" : ""}`}
        title="Chat (⌘⇧A)"
        onClick={onToggleChat}
      >
        <Icon name="message" />
        Chat
      </button>

      <OpacitySlider value={opacityPct} onChange={onOpacityChange} />

      <div className="ov-toolbar__spacer" data-tauri-drag-region />

      {/* Collapse hides only the answer body below. Screen-share hiding is
          a Settings toggle. ⌘⇧H hides the whole overlay from your screen. */}
      <button
        type="button"
        className={`ov-pill ov-pill--icon${collapsed ? " ov-pill--active" : ""}`}
        title="Collapse (⌘H)"
        aria-label="Collapse"
        onClick={onToggleCollapse}
      >
        <Icon name="chevronDown" />
      </button>

      <button
        ref={moreButtonRef}
        type="button"
        className={`ov-pill ov-pill--icon${moreOpen ? " ov-pill--active" : ""}`}
        title="More"
        aria-label="More"
        aria-expanded={moreOpen}
        onClick={onToggleMore}
      >
        <Icon name="more" />
      </button>

      <button
        type="button"
        className="ov-timer"
        title="End Session. This session ends when inactivity is detected."
        onClick={onEndClick}
      >
        <span className="numeral">{elapsedLabel}</span>
      </button>
    </div>
  );
}
